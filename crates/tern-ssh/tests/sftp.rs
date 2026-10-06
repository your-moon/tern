//! SFTP against an in-process server backed by a temp directory.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};

use common::{Shell, serve, serve_sftp, spec, start};
use tern_ssh::{Progress, SftpError};

/// Bytes that differ from one position to the next, 300001 long: not a multiple of any chunk.
fn pattern() -> Vec<u8> {
    (0..300_001u32)
        .map(|i| (i.wrapping_mul(31) % 251) as u8)
        .collect()
}

struct Env {
    remote: tempfile::TempDir,
    local: tempfile::TempDir,
    sftp: tern_ssh::Sftp,
    _live: common::Live,
    _keys: tempfile::TempDir,
}

async fn env() -> Env {
    let remote = tempfile::tempdir().unwrap();
    std::fs::create_dir(remote.path().join("home")).unwrap();
    let keys = tempfile::tempdir().unwrap();
    let server = serve_sftp(remote.path().to_path_buf()).await;
    let live = start(spec(server.port, &keys)).await.unwrap();
    let sftp = live.handle.open_sftp().await.unwrap();
    Env {
        remote,
        local: tempfile::tempdir().unwrap(),
        sftp,
        _live: live,
        _keys: keys,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn home_is_where_dot_resolves() {
    let e = env().await;
    assert_eq!(e.sftp.home().await.unwrap(), "/home");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_puts_folders_first_then_sorts_by_name() {
    let e = env().await;
    let home = e.remote.path().join("home");
    std::fs::write(home.join("b.txt"), b"12345").unwrap();
    std::fs::write(home.join("A.txt"), b"").unwrap();
    std::fs::create_dir(home.join("zeta")).unwrap();
    std::fs::create_dir(home.join("alpha")).unwrap();
    let got = e.sftp.list("/home").await.unwrap();
    let names: Vec<_> = got.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["alpha", "zeta", "A.txt", "b.txt"]);
    assert_eq!(
        got.iter().map(|x| x.is_dir).collect::<Vec<_>>(),
        [true, true, false, false]
    );
    let b = &got[3];
    assert_eq!((b.size, b.path.as_str()), (5, "/home/b.txt"));
    assert!(b.modified.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn download_copies_every_byte_and_reports_progress() {
    let e = env().await;
    let data = pattern();
    std::fs::write(e.remote.path().join("home/big.bin"), &data).unwrap();
    let dest = e.local.path().join("big.bin");
    let seen = Arc::new(Mutex::new(Vec::<Progress>::new()));
    let log = seen.clone();
    let n = e
        .sftp
        .download("/home/big.bin", &dest, move |p| log.lock().unwrap().push(p))
        .await
        .unwrap();
    assert_eq!(n, data.len() as u64);
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    assert!(!e.local.path().join("big.bin.part").exists());
    let seen = seen.lock().unwrap();
    assert!(seen.len() > 3, "progress per chunk, got {}", seen.len());
    assert!(seen.windows(2).all(|w| w[0].done < w[1].done));
    assert!(seen.iter().all(|p| p.total == data.len() as u64));
    assert_eq!(seen.last().unwrap().done, data.len() as u64);
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_copies_every_byte_and_reports_progress() {
    let e = env().await;
    let data = pattern();
    let src = e.local.path().join("up.bin");
    std::fs::write(&src, &data).unwrap();
    let seen = Arc::new(Mutex::new(Vec::<Progress>::new()));
    let log = seen.clone();
    let n = e
        .sftp
        .upload(&src, "/home/up.bin", move |p| log.lock().unwrap().push(p))
        .await
        .unwrap();
    assert_eq!(n, data.len() as u64);
    assert_eq!(
        std::fs::read(e.remote.path().join("home/up.bin")).unwrap(),
        data
    );
    let seen = seen.lock().unwrap();
    assert!(seen.len() > 3);
    assert_eq!(seen.last().unwrap().done, data.len() as u64);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_download_leaves_no_file_and_keeps_the_old_one() {
    let e = env().await;
    let dest = e.local.path().join("keep.txt");
    std::fs::write(&dest, b"old").unwrap();
    let err = e
        .sftp
        .download("/home/missing.txt", &dest, |_| {})
        .await
        .unwrap_err();
    assert_eq!(err, SftpError::NotFound("/home/missing.txt".into()));
    assert_eq!(
        err.to_string(),
        "/home/missing.txt: no such file or directory"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    assert!(!e.local.path().join("keep.txt.part").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn listing_a_missing_directory_and_uploading_a_missing_file_say_which() {
    let e = env().await;
    assert_eq!(
        e.sftp.list("/nope").await.unwrap_err(),
        SftpError::NotFound("/nope".into())
    );
    let err = e
        .sftp
        .upload(&e.local.path().join("absent"), "/home/x", |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, SftpError::Local { .. }), "{err:?}");
    assert!(!e.remote.path().join("home/x").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_without_sftp_is_reported_as_such() {
    let server = serve(Shell::Echo).await;
    let dir = tempfile::tempdir().unwrap();
    let live = start(spec(server.port, &dir)).await.unwrap();
    let err = live.handle.open_sftp().await.unwrap_err();
    assert_eq!(err, SftpError::NotOffered);
    live.handle.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_that_breaks_midway_leaves_neither_part_nor_target() {
    let e = env().await;
    std::fs::write(e.remote.path().join("home/flaky"), pattern()).unwrap();
    let dest = e.local.path().join("flaky");
    let err = e
        .sftp
        .download("/home/flaky", &dest, |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, SftpError::Other(_)), "{err:?}");
    assert!(!dest.exists());
    assert!(!e.local.path().join("flaky.part").exists());
}
