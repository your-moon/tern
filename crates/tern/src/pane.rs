// Adapted from zeron crates/ui/src/shell.rs (WidthTween, sidebar resize handle, DragGhost) (MIT).
//! Sidebar width animation and the drag that resizes it.

use std::time::{Duration, Instant};

use gpui::{Context, Empty, IntoElement, Render, Window};

use crate::settings::{SIDEBAR_MAX, SIDEBAR_MIN};

/// zeron's pane transition: 200 ms ease-out.
pub const TWEEN: Duration = Duration::from_millis(200);
/// Half the width of the invisible grab strip centred on the sidebar edge.
pub const HANDLE_HALF_WIDTH: f32 = 10.0;

#[derive(Debug, Clone, Copy)]
pub struct WidthTween {
    pub from: f32,
    pub to: f32,
    pub started: Instant,
}

impl WidthTween {
    pub fn new(from: f32, to: f32) -> Self {
        Self {
            from,
            to,
            started: Instant::now(),
        }
    }

    /// The width now, or `None` once the tween has finished.
    pub fn sample(&self, now: Instant) -> Option<f32> {
        let elapsed = now.saturating_duration_since(self.started);
        (elapsed < TWEEN).then(|| width_at(self.from, self.to, elapsed))
    }
}

/// Ease-out cubic from `from` to `to` over [`TWEEN`].
pub fn width_at(from: f32, to: f32, elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / TWEEN.as_secs_f32()).clamp(0.0, 1.0);
    let eased = 1.0 - (1.0 - t).powi(3);
    from + (to - from) * eased
}

/// Sidebar width for a pointer at window x `x`: the sidebar starts at the window's left edge.
pub fn dragged_width(x: f32) -> f32 {
    x.clamp(SIDEBAR_MIN, SIDEBAR_MAX)
}

/// Drag payload for the sidebar handle.
pub struct SidebarResize;

/// gpui needs a view to show under the pointer while dragging; the resize shows nothing.
pub struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tween_starts_at_from_ends_at_to_and_is_front_loaded() {
        assert_eq!(width_at(256.0, 0.0, Duration::ZERO), 256.0);
        assert_eq!(width_at(256.0, 0.0, TWEEN), 0.0);
        // Ease-out covers more than half the distance in the first half of the time.
        let half = width_at(0.0, 100.0, TWEEN / 2);
        assert!(half > 50.0 && half < 100.0, "{half}");
    }

    #[test]
    fn finished_tween_samples_none() {
        let tween = WidthTween::new(0.0, 256.0);
        assert!(tween.sample(tween.started).is_some());
        assert!(tween.sample(tween.started + TWEEN).is_none());
    }

    #[test]
    fn drag_is_clamped_to_the_sidebar_range() {
        assert_eq!(dragged_width(50.0), SIDEBAR_MIN);
        assert_eq!(dragged_width(300.0), 300.0);
        assert_eq!(dragged_width(1200.0), SIDEBAR_MAX);
    }
}
