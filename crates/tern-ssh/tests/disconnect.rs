//! Why a session ended, against an in-process server and a relay that can drop the network.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use common::{Shell, proxy, serve, spec, start};
use tern_ssh::Disconnect;

#[tokio::test(flavor = "multi_thread")]
async fn typing_exit_is_not_a_network_drop() {
    let server = serve(Shell::Exit(3)).await;
    let dir = tempfile::tempdir().unwrap();
    let live = start(spec(server.port, &dir)).await.unwrap();
    let (error, reason) = live.closed().await;
    assert_eq!(error, None);
    assert_eq!(reason, Disconnect::Exited(3));
    assert!(!reason.is_network());
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_locally_is_local() {
    let server = serve(Shell::Echo).await;
    let dir = tempfile::tempdir().unwrap();
    let live = start(spec(server.port, &dir)).await.unwrap();
    live.handle.close();
    let (_, reason) = live.closed().await;
    assert_eq!(reason, Disconnect::Local);
    assert!(!reason.is_network());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_silent_network_ends_as_timeout() {
    let server = serve(Shell::Echo).await;
    let relay = proxy(server.port).await;
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(relay.port, &dir);
    s.server_alive_interval = Duration::from_millis(100);
    s.server_alive_count_max = 2;
    let live = start(s).await.unwrap();
    relay.freeze();
    let (error, reason) = live.closed().await;
    assert_eq!(reason, Disconnect::Timeout);
    assert!(reason.is_network());
    assert!(error.is_none() || error.unwrap().contains("timed out"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reset_connection_ends_as_reset() {
    let server = serve(Shell::Echo).await;
    let relay = proxy(server.port).await;
    let dir = tempfile::tempdir().unwrap();
    let live = start(spec(relay.port, &dir)).await.unwrap();
    relay.reset();
    let (_, reason) = live.closed().await;
    assert_eq!(reason, Disconnect::Reset);
    assert!(reason.is_network());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_hangup_is_server_closed() {
    let server = serve(Shell::Hangup).await;
    let dir = tempfile::tempdir().unwrap();
    let live = start(spec(server.port, &dir)).await.unwrap();
    let (_, reason) = live.closed().await;
    assert_eq!(reason, Disconnect::ServerClosed);
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_listening_is_unreachable() {
    let gone = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let dir = tempfile::tempdir().unwrap();
    let (error, reason) = start(spec(gone, &dir)).await.err().unwrap();
    assert_eq!(reason, Disconnect::Unreachable);
    assert!(error.unwrap().contains("refused the connection"));
}

#[tokio::test(flavor = "multi_thread")]
async fn keep_alive_off_does_not_time_out_a_quiet_link() {
    let server = serve(Shell::Echo).await;
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(server.port, &dir);
    s.server_alive_interval = Duration::ZERO;
    let live = start(s).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    // Latency probes keep flowing; only a Closed would mean the quiet link timed out.
    while let Ok(ev) = live.events.try_recv() {
        assert!(matches!(ev, tern_ssh::SessionEvent::Latency(_)), "{ev:?}");
    }
    live.handle.close();
}
