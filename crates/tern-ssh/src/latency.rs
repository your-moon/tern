//! Round-trip time of the live connection, probed with our own `keepalive@openssh.com`
//! global request so it does not depend on the user's ServerAliveInterval (often off).

use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::client::{Handle, Handler};
use tokio::task::JoinHandle;

use crate::SessionEvent;

/// Gap between probes while the session is open.
const PROBE_EVERY: Duration = Duration::from_secs(5);
/// Weight of the newest sample: a spike shows within a probe or two without the figure jittering.
const ALPHA: f64 = 0.3;

/// Folds `sample` into the running figure (`None` before the first sample).
pub(crate) fn smooth(prev: Option<Duration>, sample: Duration) -> Duration {
    match prev {
        None => sample,
        Some(p) => {
            Duration::from_secs_f64(ALPHA * sample.as_secs_f64() + (1.0 - ALPHA) * p.as_secs_f64())
        }
    }
}

/// Aborts the probe task when the session ends.
pub(crate) struct Probe(JoinHandle<()>);

impl Drop for Probe {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Starts probing a logged-in session; each reply sends a [`SessionEvent::Latency`].
pub(crate) fn start<H: Handler + 'static>(
    session: Arc<Handle<H>>,
    events: async_channel::Sender<SessionEvent>,
) -> Probe {
    Probe(tokio::spawn(async move {
        let mut smoothed = None;
        loop {
            let sent = Instant::now();
            match session
                .send_global_request("keepalive@openssh.com", &[], true)
                .await
            {
                // Servers that do not know the request refuse it; the refusal is still a round trip.
                Ok(_) | Err(russh::Error::RequestDenied) => {}
                Err(_) => return,
            }
            let now = smooth(smoothed, sent.elapsed());
            smoothed = Some(now);
            if events.send(SessionEvent::Latency(now)).await.is_err() {
                return;
            }
            tokio::time::sleep(PROBE_EVERY).await;
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_sample_is_taken_as_is() {
        assert_eq!(
            smooth(None, Duration::from_millis(40)),
            Duration::from_millis(40)
        );
    }

    #[test]
    fn a_new_sample_moves_the_figure_three_tenths_of_the_way() {
        let up = smooth(Some(Duration::from_millis(100)), Duration::from_millis(200));
        assert_eq!(up.as_micros(), 130_000);
        let down = smooth(Some(Duration::from_millis(200)), Duration::from_millis(100));
        assert_eq!(down.as_micros(), 170_000);
    }
}
