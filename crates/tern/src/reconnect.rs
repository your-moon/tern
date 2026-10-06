//! When a dropped session dials again by itself, and how long it waits between tries. Pure
//! decisions only; `session.rs` owns the timer and the terminal text.

use std::time::Duration;

use tern_ssh::Disconnect;

/// Waits between tries; the last one repeats until the user stops it.
const DELAYS: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
];

/// Only a link that broke is retried, and only for a host that has answered before: a first
/// connect that never worked (wrong address, firewall) is a setup problem, so it stops at the
/// message and waits for Enter instead of dialling a dead address forever.
pub fn should_retry(reason: &Disconnect, ever_connected: bool) -> bool {
    reason.is_network() && ever_connected
}

/// Escalating delays that start over after a connection succeeds.
#[derive(Debug, Default)]
pub struct Backoff {
    tries: usize,
}

impl Backoff {
    pub fn next_delay(&mut self) -> Duration {
        let d = DELAYS[self.tries.min(DELAYS.len() - 1)];
        self.tries += 1;
        d
    }

    pub fn reset(&mut self) {
        self.tries = 0;
    }
}

/// What a keystroke means while a retry is counting down.
#[derive(Debug, PartialEq, Eq)]
pub enum Key {
    RetryNow,
    Stop,
    Other,
}

pub fn key(input: &[u8]) -> Key {
    if input.contains(&b'\r') {
        Key::RetryNow
    } else if input == b"\x1b" {
        Key::Stop
    } else {
        Key::Other
    }
}

/// A short cause for the notice line.
pub fn label(reason: &Disconnect) -> &'static str {
    match reason {
        Disconnect::Timeout => "timeout",
        Disconnect::Reset => "connection reset",
        Disconnect::ServerClosed => "closed by server",
        Disconnect::Unreachable => "unreachable",
        _ => "closed",
    }
}

/// The countdown line, redrawn in place every second.
pub fn notice(label: &str, secs: u64) -> String {
    format!(
        "Connection lost ({label}). Reconnecting in {secs}s\u{2026} (Enter to retry now, Esc to stop)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_network_causes_of_a_session_that_worked_retry() {
        let network = [
            Disconnect::Timeout,
            Disconnect::Reset,
            Disconnect::ServerClosed,
            Disconnect::Unreachable,
        ];
        let final_ = [
            Disconnect::Exited(0),
            Disconnect::Exited(255),
            Disconnect::Signaled("KILL".into()),
            Disconnect::Local,
            Disconnect::Failed,
        ];
        for r in &network {
            assert!(should_retry(r, true), "{r:?}");
            assert!(!should_retry(r, false), "{r:?} before ever connecting");
        }
        for r in &final_ {
            assert!(!should_retry(r, true), "{r:?}");
        }
    }

    #[test]
    fn delays_escalate_cap_and_reset() {
        let mut b = Backoff::default();
        let secs: Vec<u64> = (0..8).map(|_| b.next_delay().as_secs()).collect();
        assert_eq!(secs, [1, 2, 5, 10, 30, 30, 30, 30]);
        b.reset();
        assert_eq!(b.next_delay().as_secs(), 1);
        assert_eq!(b.next_delay().as_secs(), 2);
    }

    #[test]
    fn keys_during_a_countdown() {
        assert_eq!(key(b"\r"), Key::RetryNow);
        assert_eq!(key(b"\x1b"), Key::Stop);
        assert_eq!(key(b"\x1b[A"), Key::Other);
        assert_eq!(key(b"q"), Key::Other);
    }

    #[test]
    fn notice_reads_as_specified() {
        assert_eq!(
            notice("timeout", 5),
            "Connection lost (timeout). Reconnecting in 5s\u{2026} (Enter to retry now, Esc to stop)"
        );
    }
}
