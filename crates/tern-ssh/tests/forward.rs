//! Local, remote and dynamic forwards carrying real bytes through an in-process server.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use common::{Shell, echo_listener, serve, spec, start};
use tern_ssh::{Forward, ForwardError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const PAYLOAD: &[u8] = b"tern: 0x01 payload \xff\x00 not a palindrome";

async fn round_trip(port: u16) -> Vec<u8> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(PAYLOAD).await.unwrap();
    let mut got = vec![0u8; PAYLOAD.len()];
    tokio::time::timeout(Duration::from_secs(10), s.read_exact(&mut got))
        .await
        .expect("echo timed out")
        .unwrap();
    got
}

async fn connected() -> (common::Server, common::Live, tempfile::TempDir) {
    let server = serve(Shell::Echo).await;
    let dir = tempfile::tempdir().unwrap();
    let live = start(spec(server.port, &dir)).await.unwrap();
    (server, live, dir)
}

fn local(dest_port: u16) -> Forward {
    Forward::Local {
        bind_host: "127.0.0.1".into(),
        bind_port: 0,
        dest_host: "127.0.0.1".into(),
        dest_port,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn local_forward_carries_bytes_to_the_destination_the_server_reaches() {
    let echo = echo_listener().await;
    let (server, live, _dir) = connected().await;
    let fwd = live.handle.start_forward(local(echo)).await.unwrap();
    assert_ne!(fwd.info().bound_port, 0);
    assert_eq!(round_trip(fwd.info().bound_port).await, PAYLOAD);
    // The tunnel went through the server, to exactly the destination asked for.
    assert_eq!(
        *server.opened.lock().unwrap(),
        [format!("127.0.0.1:{echo}")]
    );
    assert_eq!(live.handle.forwards(), [fwd.info().clone()]);
    live.handle.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn local_forward_listens_on_loopback_only_by_default() {
    let echo = echo_listener().await;
    let (_server, live, _dir) = connected().await;
    let f = Forward::parse_local(&format!("0 127.0.0.1:{echo}")).unwrap();
    let Forward::Local { ref bind_host, .. } = f else {
        panic!()
    };
    assert_eq!(bind_host, "127.0.0.1");
    let fwd = live.handle.start_forward(f).await.unwrap();
    // Bound to 127.0.0.1: reachable there, and the same port is not open on ::1.
    assert_eq!(round_trip(fwd.info().bound_port).await, PAYLOAD);
    assert!(
        TcpStream::connect(("::1", fwd.info().bound_port))
            .await
            .is_err()
    );
    live.handle.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_busy_port_is_reported_not_swallowed() {
    let echo = echo_listener().await;
    let (_server, live, _dir) = connected().await;
    let first = live.handle.start_forward(local(echo)).await.unwrap();
    let again = Forward::Local {
        bind_host: "127.0.0.1".into(),
        bind_port: first.info().bound_port,
        dest_host: "127.0.0.1".into(),
        dest_port: echo,
    };
    let err = live.handle.start_forward(again).await.unwrap_err();
    assert!(matches!(err, ForwardError::Listen { .. }), "{err:?}");
    assert!(err.to_string().contains("already in use"), "{err}");
    live.handle.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_forward_closes_its_listener_and_lists_nothing() {
    let echo = echo_listener().await;
    let (_server, live, _dir) = connected().await;
    let fwd = live.handle.start_forward(local(echo)).await.unwrap();
    let port = fwd.info().bound_port;
    fwd.stop();
    assert!(live.handle.forwards().is_empty());
    let mut closed = false;
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_err() {
            closed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(closed, "listener still accepting after stop()");
    fwd.stop();
    live.handle.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_forward_brings_the_servers_port_back_to_a_local_address() {
    let echo = echo_listener().await;
    let (_server, live, _dir) = connected().await;
    let fwd = live
        .handle
        .start_forward(Forward::Remote {
            bind_host: "localhost".into(),
            bind_port: 0,
            dest_host: "127.0.0.1".into(),
            dest_port: echo,
        })
        .await
        .unwrap();
    let remote_port = fwd.info().bound_port;
    assert_ne!(remote_port, 0);
    // The test server listens on 127.0.0.1:remote_port and opens forwarded-tcpip channels.
    assert_eq!(round_trip(remote_port).await, PAYLOAD);
    live.handle.close();
}

/// Minimal SOCKS5 client: CONNECT to `addr`, returns the stream and the reply code.
async fn socks_connect(proxy: u16, addr: &[u8]) -> (TcpStream, u8) {
    let mut s = TcpStream::connect(("127.0.0.1", proxy)).await.unwrap();
    s.write_all(&[5, 1, 0]).await.unwrap();
    let mut method = [0u8; 2];
    s.read_exact(&mut method).await.unwrap();
    assert_eq!(method, [5, 0]);
    let mut req = vec![5, 1, 0];
    req.extend(addr);
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 10];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[0], 5);
    (s, rep[1])
}

fn dynamic() -> Forward {
    Forward::Dynamic {
        bind_host: "127.0.0.1".into(),
        bind_port: 0,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn socks5_connect_by_ipv4_and_by_name_goes_through_the_server() {
    let echo = echo_listener().await;
    let (server, live, _dir) = connected().await;
    let fwd = live.handle.start_forward(dynamic()).await.unwrap();
    let proxy = fwd.info().bound_port;

    let mut v4 = vec![1, 127, 0, 0, 1];
    v4.extend(echo.to_be_bytes());
    let (mut s, code) = socks_connect(proxy, &v4).await;
    assert_eq!(code, 0);
    s.write_all(PAYLOAD).await.unwrap();
    let mut got = vec![0u8; PAYLOAD.len()];
    s.read_exact(&mut got).await.unwrap();
    assert_eq!(got, PAYLOAD);

    let mut name = vec![3, 9];
    name.extend(b"localhost");
    name.extend(echo.to_be_bytes());
    let (mut s, code) = socks_connect(proxy, &name).await;
    assert_eq!(code, 0);
    s.write_all(PAYLOAD).await.unwrap();
    s.read_exact(&mut got).await.unwrap();
    assert_eq!(got, PAYLOAD);

    // The server, not this machine, opened both connections, and the name reached it unresolved.
    assert_eq!(
        *server.opened.lock().unwrap(),
        [format!("127.0.0.1:{echo}"), format!("localhost:{echo}")]
    );
    live.handle.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn socks5_reports_a_destination_the_server_cannot_reach() {
    let gone = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let (_server, live, _dir) = connected().await;
    let fwd = live.handle.start_forward(dynamic()).await.unwrap();
    let mut v4 = vec![1, 127, 0, 0, 1];
    v4.extend(gone.to_be_bytes());
    let (_, code) = socks_connect(fwd.info().bound_port, &v4).await;
    assert_eq!(code, 5, "connection refused");
    live.handle.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn starting_a_forward_after_the_session_ended_fails_cleanly() {
    let (_server, live, _dir) = connected().await;
    live.handle.close();
    let _ = live.closed().await;
    let err = live.handle.start_forward(dynamic()).await.unwrap_err();
    assert_eq!(err, ForwardError::Closed);
}
