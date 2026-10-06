#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use alacritty_terminal::vte::ansi::CursorShape;

use super::*;
use crate::options::CursorStyleSetting;
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
