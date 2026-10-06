//! The window's frame: one composition, the same with and without a wallpaper.
//!
//! Layers, back to front, each drawn once:
//! 1. the base at the window root: the opaque shell colour, or, with no wallpaper, the macOS
//!    frost (the window's own blurred appearance behind the shell colour at `GLASS_ALPHA`);
//! 2. when a wallpaper fills the window, its blurred copy, cover-fit to the window, one element;
//! 3. tiles side by side, edge to edge, with no margins, cards or gaps: the titlebar band across
//!    the top, then sidebar | main (or Settings nav | page). A tile is only a tint at the panel
//!    alpha over the layers behind it, so panels never carry a picture of their own;
//! 4. one neutral 1 px hairline on the sidebar's right edge, which moves with it as it tweens.
//!
//! The sharp picture is only the empty view's hero, laid out inside the main tile. Sidebar width
//! is held in whole device pixels so no edge lands between two.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Hsla, IntoElement, ParentElement, Styled, Window, WindowBackgroundAppearance, div,
    px,
};

use super::Shell;

/// `value` (logical px) rounded to a whole number of device pixels at `scale`.
pub(crate) fn snap(value: f32, scale: f32) -> f32 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    (value * scale).round() / scale
}

/// What the frame remembers between frames.
pub(super) struct FrameState {
    /// The window's scale factor at its last render, for snapping widths outside `render`.
    pub(super) scale: f32,
    /// The window appearance last set, so it is only touched when it changes.
    blurred: Option<bool>,
}

impl Default for FrameState {
    fn default() -> Self {
        Self {
            scale: 1.0,
            blurred: None,
        }
    }
}

impl Shell {
    /// Records the scale factor and switches the window between its blurred appearance (no
    /// wallpaper: the desktop shows through the frost) and an opaque one (a wallpaper is set:
    /// nothing behind the window may show).
    pub(super) fn sync_frame(&mut self, window: &mut Window) {
        self.frame.scale = window.scale_factor();
        let blurred = !self.has_wallpaper();
        if self.frame.blurred != Some(blurred) {
            self.frame.blurred = Some(blurred);
            window.set_background_appearance(if blurred {
                WindowBackgroundAppearance::Blurred
            } else {
                WindowBackgroundAppearance::Opaque
            });
        }
    }

    /// The window root's colour: always opaque under a wallpaper, frost otherwise.
    pub(super) fn window_base(&self) -> Hsla {
        if self.has_wallpaper() {
            self.theme.shell
        } else {
            self.theme.surface()
        }
    }

    /// The blurred picture across the window, drawn once; empty unless the picture fills it.
    pub(super) fn backdrop(&self) -> Vec<AnyElement> {
        use gpui::StyledImage as _;
        let Some(blurred) = self.blurred_backdrop() else {
            return Vec::new();
        };
        vec![
            gpui::img(blurred)
                .absolute()
                .inset_0()
                .size_full()
                .object_fit(gpui::ObjectFit::Cover)
                .opacity(self.settings.wallpaper_hero_opacity)
                .into_any_element(),
        ]
    }

    /// The tint a tile of colour `colour` lays over what is behind it: the panel alpha over a
    /// full-window picture, nothing otherwise (the window base already is that colour).
    pub(super) fn tile_tint(&self, colour: Hsla) -> Option<Hsla> {
        self.window_fill().map(|alpha| colour.opacity(alpha))
    }

    /// A fixed-width tile at the left of the body (the sidebar, or Settings' nav) with the
    /// separator on its right edge. Clipped to `width`, so collapsing never reflows its rows.
    pub(super) fn side_tile(&self, width: f32, content: impl IntoElement) -> gpui::Div {
        div()
            .flex_none()
            .relative()
            .h_full()
            .w(px(width))
            .overflow_hidden()
            .when_some(self.tile_tint(self.theme.shell), |el, tint| el.bg(tint))
            .child(content)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(px(1.))
                    .bg(self.theme.hairline),
            )
    }

    /// The main tile: fills what the side tile leaves. When it shows a terminal over a
    /// full-window wallpaper it paints nothing, as the terminal draws its own glass at the same
    /// alpha; anything else it shows (Settings, the empty view) sits on the tile's tint.
    pub(super) fn main_tile(&self, panel: Hsla) -> gpui::Div {
        let shows_terminal = !self.tabs.is_empty() && self.settings_page.is_none();
        let terminal_glass = shows_terminal && self.window_fill().is_some();
        div()
            .flex_1()
            .min_w_0()
            .relative()
            .overflow_hidden()
            .flex()
            .flex_col()
            .when(!terminal_glass, |el| {
                el.bg(self.tile_tint(panel).unwrap_or(panel))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::snap;

    #[test]
    fn snap_lands_on_whole_device_pixels() {
        for (value, scale) in [(256.4, 1.0), (256.4, 2.0), (257.3, 2.0), (300.7, 1.5)] {
            let device = snap(value, scale) * scale;
            assert!((device - device.round()).abs() < 1e-3, "{value}@{scale}");
        }
    }

    #[test]
    fn snap_rounds_to_the_nearest_not_down() {
        assert_eq!(snap(256.4, 1.0), 256.0);
        assert_eq!(snap(256.6, 1.0), 257.0);
        // At 2x half a point is a whole device pixel; a quarter rounds to the nearer edge.
        assert_eq!(snap(256.5, 2.0), 256.5);
        assert_eq!(snap(256.25, 2.0), 256.5);
        assert_eq!(snap(256.2, 2.0), 256.0);
    }

    #[test]
    fn snap_survives_a_bad_scale() {
        assert_eq!(snap(10.4, 0.0), 10.0);
        assert_eq!(snap(10.4, f32::NAN), 10.0);
    }
}
