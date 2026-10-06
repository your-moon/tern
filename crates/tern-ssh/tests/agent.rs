//! Agent forwarding: the server opens an agent channel and it reaches (or does not reach) a
//! local socket. The socket here is a test echo, standing in for ssh-agent.
// The stand-in agent is a Unix socket; Windows forwards to a named pipe instead.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;
use std::time::Duration;

use common::{AgentProbe, Shell, serve_with, spec, start};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

/// Echoes whatever it is sent, as a stand-in agent. Returns once listening.
fn fake_agent(path: &Path) {
    let listener = UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 256];
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 || s.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
}

async fn settle(log: &std::sync::Mutex<Vec<String>>) -> Vec<String> {
    for _ in 0..40 {
        if !log.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    log.lock().unwrap().clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_channels_are_wired_to_the_local_agent_when_asked_for() {
    let server = serve_with(Shell::Echo, AgentProbe::WhenAsked).await;
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("agent.sock");
    fake_agent(&sock);
    let mut s = spec(server.port, &dir);
    s.forward_agent = true;
    s.agent_socket = Some(sock);
    let live = start(s).await.unwrap();
    assert_eq!(settle(&server.agent_log).await, ["agent-ping"]);
    live.handle.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn without_forward_agent_the_server_is_never_asked_and_a_forced_channel_is_refused() {
    // Never asked: a server that only opens the channel after a request opens nothing.
    let polite = serve_with(Shell::Echo, AgentProbe::WhenAsked).await;
    let dir = tempfile::tempdir().unwrap();
    let live = start(spec(polite.port, &dir)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(polite.agent_log.lock().unwrap().is_empty());
    live.handle.close();

    // A server that opens one anyway gets a refusal, not the agent.
    let hostile = serve_with(Shell::Echo, AgentProbe::Always).await;
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("agent.sock");
    fake_agent(&sock);
    let mut s = spec(hostile.port, &dir);
    s.agent_socket = Some(sock);
    let live = start(s).await.unwrap();
    assert_eq!(settle(&hostile.agent_log).await, ["refused"]);
    live.handle.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_agent_socket_does_not_break_the_login() {
    let server = serve_with(Shell::Echo, AgentProbe::WhenAsked).await;
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(server.port, &dir);
    s.forward_agent = true;
    s.agent_socket = Some(dir.path().join("nothing-here.sock"));
    let live = start(s).await.unwrap();
    // The channel was accepted, then had nowhere to go: the server sees no answer.
    assert_eq!(settle(&server.agent_log).await.len(), 1);
    assert_ne!(server.agent_log.lock().unwrap()[0], "agent-ping");
    live.handle.close();
}
