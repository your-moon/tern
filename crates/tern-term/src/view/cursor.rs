//! Cursor blink and the options setter for [`TerminalView`].

use super::*;

/// Blink half-period, and how long typing holds the cursor solid.
pub(super) const CURSOR_BLINK_MS: u64 = 500;

/// The cursor's next on/off state at a blink tick. A cursor that does not
/// blink, or whose owner typed within the last period, stays on; otherwise it
/// flips.
pub(super) fn blink_phase(on: bool, blinking: bool, since_input: Duration) -> bool {
    if !blinking || since_input < Duration::from_millis(CURSOR_BLINK_MS) {
        true
    } else {
        !on
    }
}

impl TerminalView {
    /// Apply behaviour options (cursor style, blink, ...). Cheap; call it
    /// whenever the app's settings change.
    pub fn set_options(&mut self, options: TerminalOptions, cx: &mut Context<Self>) {
        self.terminal.update(cx, |t, cx| {
            t.set_default_cursor(options.cursor_style, options.cursor_blink);
            cx.notify();
        });
        self.options = options;
        self.cursor_on = true;
        cx.notify();
    }

    pub fn options(&self) -> &TerminalOptions {
        &self.options
    }

    /// Whether the blinking cursor is currently in its visible phase.
    pub(crate) fn cursor_on(&self) -> bool {
        self.cursor_on
    }

    /// Typing holds the cursor solid, then blinking resumes.
    pub(super) fn note_input(&mut self) {
        self.last_input = Instant::now();
        self.cursor_on = true;
    }

    pub(super) fn blink_loop(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(CURSOR_BLINK_MS))
                    .await;
                if this.update(cx, |view, cx| view.blink_tick(cx)).is_err() {
                    break;
                }
            }
        })
    }

    fn blink_tick(&mut self, cx: &mut Context<Self>) {
        let blinking = self.terminal.read(cx).cursor_blinking();
        let on = blink_phase(self.cursor_on, blinking, self.last_input.elapsed());
        if on != self.cursor_on {
            self.cursor_on = on;
            cx.notify();
        }
    }
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
