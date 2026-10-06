//! The round-trip time of a live session, against the in-process server.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use common::{Shell, serve, spec, start};
use tern_ssh::SessionEvent;

#[tokio::test(flavor = "multi_thread")]
async fn a_live_session_reports_a_plausible_round_trip() {
    let server = serve(Shell::Echo).await;
    let dir = tempfile::tempdir().unwrap();
    let live = start(spec(server.port, &dir)).await.unwrap();
    let wait = async {
        loop {
            if let SessionEvent::Latency(rtt) = live.events.recv().await.expect("no Latency event")
            {
                return rtt;
            }
        }
    };
    let rtt = tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .expect("no latency within 10 s");
    // Loopback: nonzero (a real round trip happened) and far under a second.
    assert!(rtt > Duration::ZERO, "{rtt:?}");
    assert!(rtt < Duration::from_secs(1), "{rtt:?}");
}
