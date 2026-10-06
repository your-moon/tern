//! The visual bell: a brief fading wash over the view.

use super::*;

/// How long the flash takes to fade out.
pub(super) const BELL_FLASH_MS: u64 = 150;
/// Wash opacity at the start of the flash.
pub(super) const BELL_PEAK_OPACITY: f32 = 0.25;

impl TerminalView {
    /// Start (or restart) a flash and schedule its removal.
    pub(super) fn flash(&mut self, cx: &mut Context<Self>) {
        self.bell_seq += 1;
        self.bell_active = true;
        self.bell_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(BELL_FLASH_MS))
                .await;
            let _ = this.update(cx, |view, cx| {
                view.bell_active = false;
                cx.notify();
            });
        }));
        cx.notify();
    }
}
