//! Cursor blink and the options setter for [`TerminalView`].
//!
//! The visual bell: a brief fading wash over the view.

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
    /// Apply behaviour options (cursor style, blink, scrollback, ...). Cheap; call it
    /// whenever the app's settings change.
    pub fn set_options(&mut self, options: TerminalOptions, cx: &mut Context<Self>) {
        self.terminal.update(cx, |t, cx| {
            t.set_default_cursor(options.cursor_style, options.cursor_blink);
            t.set_scrollback(options.scrollback_lines);
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use alacritty_terminal::vte::ansi::CursorShape;

    use super::*;
    use crate::config::CursorStyleSetting;
    use crate::terminal::Terminal;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn blinking_cursor_flips_once_idle() {
        assert!(!blink_phase(true, true, ms(900)));
        assert!(blink_phase(false, true, ms(900)));
    }

    #[test]
    fn typing_holds_the_cursor_solid() {
        assert!(blink_phase(false, true, ms(100)), "recent input forces on");
        assert!(!blink_phase(true, true, ms(500)), "resumes after a period");
    }

    #[test]
    fn steady_cursor_never_goes_dark() {
        assert!(blink_phase(false, false, ms(5_000)));
        assert!(blink_phase(true, false, ms(5_000)));
    }

    #[test]
    fn settings_map_to_alacritty_shapes() {
        assert_eq!(CursorStyleSetting::Block.shape(), CursorShape::Block);
        assert_eq!(CursorStyleSetting::Bar.shape(), CursorShape::Beam);
        assert_eq!(
            CursorStyleSetting::Underline.shape(),
            CursorShape::Underline
        );
    }

    fn shape(t: &Terminal) -> (CursorShape, bool) {
        (t.cursor().unwrap().shape, t.cursor_blinking())
    }

    #[test]
    fn decscusr_from_the_remote_picks_shape_and_blink() {
        let mut t = Terminal::new(10, 2);
        assert_eq!(shape(&t), (CursorShape::Block, false));
        for (seq, want) in [
            (&b"\x1b[1 q"[..], (CursorShape::Block, true)),
            (b"\x1b[2 q", (CursorShape::Block, false)),
            (b"\x1b[3 q", (CursorShape::Underline, true)),
            (b"\x1b[4 q", (CursorShape::Underline, false)),
            (b"\x1b[5 q", (CursorShape::Beam, true)),
            (b"\x1b[6 q", (CursorShape::Beam, false)),
        ] {
            t.process(seq);
            assert_eq!(shape(&t), want, "{seq:?}");
        }
    }

    #[test]
    fn default_setting_applies_and_decscusr_zero_returns_to_it() {
        let mut t = Terminal::new(10, 2);
        t.set_default_cursor(CursorStyleSetting::Bar, true);
        assert_eq!(shape(&t), (CursorShape::Beam, true));
        t.process(b"\x1b[4 q");
        assert_eq!(shape(&t), (CursorShape::Underline, false));
        t.process(b"\x1b[0 q");
        assert_eq!(shape(&t), (CursorShape::Beam, true));
    }

    #[test]
    fn changing_options_does_not_emit_a_title_event() {
        let mut t = Terminal::new(10, 2);
        t.set_default_cursor(CursorStyleSetting::Underline, false);
        assert!(t.process(b"x").is_empty());
    }
}
