// Adapted from zeron crates/ui/src/motion.rs "Hover color fades" (HOVER_FADE line 466,
// EASE_TAILWIND line 463, FadeEntry/HoverFades, hover_t, set_hover, hover_listener, mix,
// hover_blend) (MIT).
//! Hover colour fades. gpui's `.hover()` styles snap the frame the pointer enters; zeron puts
//! Tailwind's `transition-colors` (150 ms, cubic-bezier(0.4, 0, 0.2, 1)) on every interactive
//! wash. gpui cannot animate a hover style, so this keeps a per-element progress, advanced
//! from wall time and read while the element is built; the shell asks for another frame
//! while any fade is mid-flight.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{App, Hsla, Rgba, SharedString, StatefulInteractiveElement, Styled, Window};

use crate::shell::CubicBezier;

/// zeron `EASE_TAILWIND`.
const EASE: CubicBezier = CubicBezier::new(0.4, 0.0, 0.2, 1.0);
/// zeron `HOVER_FADE`: CSS `transition-colors`, 150 ms.
pub const HOVER_FADE: Duration = Duration::from_millis(150);

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// One element's fade: progress runs `origin` to `target`, re-anchored at the current value
/// whenever the pointer flips direction mid-flight so the blend stays continuous.
#[derive(Debug, Clone, Copy)]
struct Entry {
    origin: f32,
    target: f32,
    started: Instant,
    /// Frame counter at the last read: an element that unmounts mid-hover never gets its leave
    /// event, so entries unread for a whole frame are dropped.
    seen: u64,
}

impl Entry {
    fn value(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started);
        if elapsed >= HOVER_FADE {
            return self.target;
        }
        let raw = elapsed.as_secs_f32() / HOVER_FADE.as_secs_f32();
        lerp(self.origin, self.target, EASE.eval(raw))
    }

    fn settled(&self, now: Instant) -> bool {
        self.origin == self.target || now.saturating_duration_since(self.started) >= HOVER_FADE
    }
}

/// Per-key hover progress. A pure core (explicit `now`), so it can be tested; the thread-local
/// wrappers below feed it wall time.
#[derive(Default)]
pub struct Fades {
    entries: HashMap<String, Entry>,
    frame: u64,
}

impl Fades {
    /// The pointer entered (`hovered`) or left the element behind `key`. Reduced motion snaps
    /// straight to the end.
    pub fn set_at(&mut self, key: &str, hovered: bool, reduced: bool, now: Instant) {
        let target = if hovered { 1.0 } else { 0.0 };
        let current = self.entries.get(key).map_or(0.0, |e| e.value(now));
        if target == 0.0 && !self.entries.contains_key(key) {
            return;
        }
        let origin = if reduced { target } else { current };
        let seen = self.frame;
        self.entries.insert(
            key.to_owned(),
            Entry {
                origin,
                target,
                started: now,
                seen,
            },
        );
    }

    /// Progress (0 to 1) for `key`; stamps the entry as live.
    pub fn value_at(&mut self, key: &str, now: Instant) -> f32 {
        let frame = self.frame;
        match self.entries.get_mut(key) {
            Some(entry) => {
                entry.seen = frame;
                entry.value(now)
            }
            None => 0.0,
        }
    }

    /// Once per frame: drops entries that are back at rest or went unread, and reports whether
    /// any fade is still moving (so frames must keep coming).
    pub fn tick_at(&mut self, now: Instant) -> bool {
        self.frame += 1;
        let frame = self.frame;
        let mut active = false;
        self.entries.retain(|_, entry| {
            if entry.seen + 1 < frame {
                return false;
            }
            let settled = entry.settled(now);
            if !settled {
                active = true;
            }
            !(settled && entry.target == 0.0)
        });
        active
    }
}

thread_local! {
    static FADES: RefCell<Fades> = RefCell::new(Fades::default());
}

/// Hover progress (0 to 1) for `key` this frame.
pub fn hover_t(key: &str) -> f32 {
    FADES.with(|f| f.borrow_mut().value_at(key, Instant::now()))
}

fn set_hover(key: &str, hovered: bool, reduced: bool) {
    FADES.with(|f| f.borrow_mut().set_at(key, hovered, reduced, Instant::now()));
}

/// An `.on_hover` listener driving the fade for `key`.
pub fn hover_listener(key: SharedString) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    move |hovered, window, cx| {
        set_hover(&key, *hovered, cx.reduce_motion());
        // `refresh` marks the window dirty; the shell render keeps frames coming while the
        // fade is mid-flight.
        window.refresh();
    }
}

/// `base` blended toward `hover` by the fade for `key`: for text and fills that cannot go
/// through `hover_fade` because the rest colour itself is animating (zeron `motion::hover_blend`).
pub fn blend(key: &str, base: Hsla, hover: Hsla) -> Hsla {
    mix(base, hover, hover_t(key))
}

/// Call once per frame, after the elements are built; true while a fade needs more frames.
pub fn fades_active() -> bool {
    FADES.with(|f| f.borrow_mut().tick_at(Instant::now()))
}

/// Blends two colours the way a browser transitions them: per-channel in sRGB with
/// premultiplied alpha, so a wash fading in from transparent brightens without passing
/// through grey.
pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 {
        return from;
    }
    if t >= 1.0 {
        return to;
    }
    let (f, g) = (Rgba::from(from), Rgba::from(to));
    let a = lerp(f.a, g.a, t);
    if a <= f32::EPSILON {
        return Hsla::from(Rgba { a: 0.0, ..g });
    }
    Hsla::from(Rgba {
        r: lerp(f.r * f.a, g.r * g.a, t) / a,
        g: lerp(f.g * f.a, g.g * g.a, t) / a,
        b: lerp(f.b * f.a, g.b * g.a, t) / a,
        a,
    })
}

/// Fades an element's background between `rest` and `hover` as the pointer enters and leaves.
/// `key` must be unique to the element and stable across frames.
pub trait HoverFade: StatefulInteractiveElement + Styled + Sized {
    fn hover_fade(self, key: impl Into<SharedString>, rest: Hsla, hover: Hsla) -> Self {
        let key = key.into();
        let bg = mix(rest, hover, hover_t(&key));
        self.on_hover(hover_listener(key)).bg(bg)
    }
}

impl<T: StatefulInteractiveElement + Styled + Sized> HoverFade for T {}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64, base: Instant) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn a_fade_runs_over_150_ms_along_the_tailwind_curve() {
        let t0 = Instant::now();
        let mut f = Fades::default();
        f.set_at("a", true, false, t0);
        assert_eq!(f.value_at("a", t0), 0.0);
        let mid = f.value_at("a", at(75, t0));
        assert!(mid > 0.7 && mid < 0.9, "{mid}");
        assert!(f.value_at("a", at(40, t0)) < mid);
        assert_eq!(f.value_at("a", at(150, t0)), 1.0);
        assert_eq!(f.value_at("a", at(900, t0)), 1.0);
    }

    #[test]
    fn leaving_mid_fade_turns_around_from_where_it_was() {
        let t0 = Instant::now();
        let mut f = Fades::default();
        f.set_at("a", true, false, t0);
        let reached = f.value_at("a", at(60, t0));
        f.set_at("a", false, false, at(60, t0));
        let just_after = f.value_at("a", at(61, t0));
        assert!(
            (just_after - reached).abs() < 0.05,
            "{reached} {just_after}"
        );
        assert_eq!(f.value_at("a", at(400, t0)), 0.0);
    }

    #[test]
    fn reduced_motion_snaps() {
        let t0 = Instant::now();
        let mut f = Fades::default();
        f.set_at("a", true, true, t0);
        assert_eq!(f.value_at("a", t0), 1.0);
    }

    #[test]
    fn a_never_hovered_element_reporting_a_leave_is_ignored() {
        let mut f = Fades::default();
        f.set_at("ghost", false, false, Instant::now());
        assert!(f.entries.is_empty());
    }

    #[test]
    fn tick_keeps_frames_coming_only_while_a_fade_moves() {
        let t0 = Instant::now();
        let mut f = Fades::default();
        f.set_at("a", true, false, t0);
        assert!(f.tick_at(at(10, t0)));
        f.value_at("a", at(20, t0));
        assert!(f.tick_at(at(30, t0)));
        f.value_at("a", at(200, t0));
        assert!(!f.tick_at(at(210, t0)));
        // Settled hovered stays (the wash must remain); settled at rest is dropped.
        assert_eq!(f.value_at("a", at(220, t0)), 1.0);
        f.set_at("a", false, false, at(220, t0));
        f.value_at("a", at(500, t0));
        f.tick_at(at(500, t0));
        assert!(f.entries.is_empty());
    }

    #[test]
    fn an_unmounted_element_does_not_leave_a_stuck_wash() {
        let t0 = Instant::now();
        let mut f = Fades::default();
        f.set_at("gone", true, false, t0);
        f.tick_at(at(10, t0));
        f.tick_at(at(20, t0));
        assert!(f.entries.is_empty());
    }

    #[test]
    fn mix_fades_a_wash_in_from_transparent_without_going_grey() {
        let clear = gpui::hsla(0.0, 0.0, 0.0, 0.0);
        let wash = gpui::hsla(0.0, 0.0, 0.92, 0.10);
        assert_eq!(mix(clear, wash, 0.0), clear);
        assert_eq!(mix(clear, wash, 1.0), wash);
        let half = mix(clear, wash, 0.5);
        assert!((half.a - 0.05).abs() < 1e-5);
        // The wash keeps its own lightness while only the alpha rises.
        assert!((half.l - 0.92).abs() < 1e-3, "{}", half.l);
    }
}
