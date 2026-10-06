//! Terminal palette. Replaces Zed's `theme` crate with a small local struct.
//!
//! The 256-color cube and grayscale ramp are computed (xterm formulas), only
//! the 16 named ANSI slots plus fg/bg/cursor/selection come from the theme.

use gpui::{Hsla, Rgba, SharedString, rgb, rgba};

use crate::terminal::CellColor;

#[derive(Clone, Debug)]
pub struct TerminalTheme {
    /// ANSI 0-7 normal, 8-15 bright.
    pub ansi: [Hsla; 16],
    pub foreground: Hsla,
    pub background: Hsla,
    /// Opacity of the view's own background fill; below 1.0 whatever is behind the terminal
    /// (a wallpaper) shows through. Cells with their own background colour stay opaque.
    pub background_alpha: f32,
    pub cursor: Hsla,
    pub selection: Hsla,
    /// Wash over find matches.
    pub search_match: Hsla,
    /// Wash over the current find match.
    pub search_current: Hsla,
    pub font_family: SharedString,
    pub font_size: f32,
    /// Line height as a multiple of the font size.
    pub line_height_ratio: f32,
}

impl Default for TerminalTheme {
    fn default() -> Self {
        let c = |hex: u32| -> Hsla { rgb(hex).into() };
        Self {
            ansi: [
                c(0x1d1f21),
                c(0xcc6666),
                c(0xb5bd68),
                c(0xf0c674),
                c(0x81a2be),
                c(0xb294bb),
                c(0x8abeb7),
                c(0xc5c8c6),
                c(0x666a70),
                c(0xff7a7a),
                c(0xc9d27f),
                c(0xffd98a),
                c(0x9bbcdc),
                c(0xcbacd3),
                c(0xa3d4cd),
                c(0xffffff),
            ],
            foreground: c(0xc5c8c6),
            background: c(0x16181a),
            background_alpha: 1.0,
            cursor: c(0xe0e0e0),
            selection: rgba(0x81a2be55).into(),
            search_match: rgba(0xf0c67455).into(),
            search_current: rgba(0xf0a030b0).into(),
            font_family: default_font_family().into(),
            font_size: 13.0,
            line_height_ratio: 1.35,
        }
    }
}

fn default_font_family() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}

/// xterm 256-color cube component levels.
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// RGB for indexed colors 16..=255 (cube and grayscale ramp).
pub fn extended_indexed_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0..=15 => (0, 0, 0),
        16..=231 => {
            let n = index as usize - 16;
            (
                CUBE_LEVELS[n / 36],
                CUBE_LEVELS[(n / 6) % 6],
                CUBE_LEVELS[n % 6],
            )
        }
        232..=255 => {
            let v = 8 + 10 * (index - 232);
            (v, v, v)
        }
    }
}

fn rgb8(r: u8, g: u8, b: u8) -> Hsla {
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
    .into()
}

impl TerminalTheme {
    /// Resolve a cell color against this palette.
    pub fn resolve(&self, color: CellColor) -> Hsla {
        match color {
            CellColor::Foreground => self.foreground,
            CellColor::Background => self.background,
            CellColor::Indexed(ix @ 0..=15) => self.ansi[ix as usize],
            CellColor::Indexed(ix) => {
                let (r, g, b) = extended_indexed_rgb(ix);
                rgb8(r, g, b)
            }
            CellColor::Rgb(r, g, b) => rgb8(r, g, b),
        }
    }

    /// 8-bit RGB for a color index in alacritty's `ColorRequest` numbering:
    /// 0-255 palette, 256 foreground, 257 background, 258 cursor.
    pub(crate) fn rgb_for_request(&self, index: usize) -> Option<(u8, u8, u8)> {
        let hsla = match index {
            0..=15 => self.ansi[index],
            16..=255 => return Some(extended_indexed_rgb(index as u8)),
            256 => self.foreground,
            257 => self.background,
            258 => self.cursor,
            _ => return None,
        };
        let c = Rgba::from(hsla);
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Some((q(c.r), q(c.g), q(c.b)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_corners() {
        assert_eq!(extended_indexed_rgb(16), (0, 0, 0));
        assert_eq!(extended_indexed_rgb(231), (255, 255, 255));
        assert_eq!(extended_indexed_rgb(196), (255, 0, 0));
        assert_eq!(extended_indexed_rgb(21), (0, 0, 255));
        assert_eq!(extended_indexed_rgb(232), (8, 8, 8));
        assert_eq!(extended_indexed_rgb(255), (238, 238, 238));
    }

    #[test]
    fn request_rgb_roundtrips_theme_background() {
        let t = TerminalTheme::default();
        assert_eq!(t.rgb_for_request(257), Some((0x16, 0x18, 0x1a)));
        assert_eq!(t.rgb_for_request(1), Some((0xcc, 0x66, 0x66)));
    }
}
