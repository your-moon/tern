//! Remote output on its way to the UI, coalesced and bounded. While the outbox is full the
//! session stops reading the channel, so russh stops reading the socket (backpressure).

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use crate::SessionEvent;

/// Largest coalesced chunk handed to the UI in one [`SessionEvent::Data`].
pub(crate) const MAX_CHUNK: usize = 64 * 1024;

/// Capacity of the event channel to the UI. Chunks are coalesced, so a short queue suffices.
pub(crate) const EVENT_QUEUE: usize = 8;

type InFlight = Pin<Box<dyn Future<Output = bool> + Send>>;

pub(crate) struct Outbox {
    tx: async_channel::Sender<SessionEvent>,
    buf: Vec<u8>,
    in_flight: Option<InFlight>,
}

impl fmt::Debug for Outbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Outbox")
            .field("buffered", &self.buf.len())
            .field("sending", &self.in_flight.is_some())
            .finish()
    }
}

impl Outbox {
    pub(crate) fn new(tx: async_channel::Sender<SessionEvent>) -> Self {
        Self {
            tx,
            buf: Vec::new(),
            in_flight: None,
        }
    }

    /// Whether the session may read more from the SSH channel.
    pub(crate) fn wants_input(&self) -> bool {
        self.buf.len() < MAX_CHUNK
    }

    pub(crate) fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Whether [`Self::progress`] has anything to do.
    pub(crate) fn has_work(&self) -> bool {
        self.in_flight.is_some() || !self.buf.is_empty()
    }

    /// Delivers one chunk; `false` once the receiver is gone. Cancel-safe: the in-flight send
    /// lives in `self`, so the next call resumes it.
    pub(crate) async fn progress(&mut self) -> bool {
        if self.in_flight.is_none() {
            if self.buf.is_empty() {
                return true;
            }
            let chunk = std::mem::take(&mut self.buf);
            let tx = self.tx.clone();
            self.in_flight = Some(Box::pin(async move {
                tx.send(SessionEvent::Data(chunk)).await.is_ok()
            }));
        }
        let delivered = match self.in_flight.as_mut() {
            Some(send) => send.await,
            None => true,
        };
        self.in_flight = None;
        delivered
    }

    pub(crate) async fn flush(&mut self) -> bool {
        while self.has_work() {
            if !self.progress().await {
                return false;
            }
        }
        true
    }

    #[cfg(test)]
    fn buffered(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn slow_consumer_keeps_buffer_bounded_and_preserves_order() {
        const READ: usize = 4 * 1024;
        const TOTAL_READS: usize = 400; // 1.6 MiB, 25x MAX_CHUNK

        let (tx, rx) = async_channel::bounded(EVENT_QUEUE);
        let consumer = tokio::spawn(async move {
            let mut got = Vec::new();
            while let Ok(ev) = rx.recv().await {
                if let SessionEvent::Data(d) = ev {
                    got.extend_from_slice(&d);
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            got
        });

        let mut outbox = Outbox::new(tx);
        let mut sent = Vec::new();
        let mut peak = 0;
        let mut reads = 0;
        while reads < TOTAL_READS || outbox.has_work() {
            tokio::select! {
                () = std::future::ready(()), if reads < TOTAL_READS && outbox.wants_input() => {
                    let read: Vec<u8> = (0..READ).map(|i| ((reads * READ + i) % 251) as u8).collect();
                    outbox.push(&read);
                    sent.extend_from_slice(&read);
                    reads += 1;
                }
                ok = outbox.progress(), if outbox.has_work() => assert!(ok),
            }
            peak = peak.max(outbox.buffered());
        }
        drop(outbox);

        let got = consumer.await.unwrap();
        assert!(peak < MAX_CHUNK + READ, "buffer peaked at {peak} bytes");
        assert_eq!(got.len(), sent.len());
        assert!(got == sent, "bytes reordered or corrupted");
    }

    #[tokio::test]
    async fn progress_reports_a_dropped_receiver() {
        let (tx, rx) = async_channel::bounded(1);
        drop(rx);
        let mut outbox = Outbox::new(tx);
        outbox.push(b"x");
        assert!(!outbox.progress().await);
    }
}
