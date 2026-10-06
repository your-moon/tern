// Adapted from zeron crates/ui/src/icons.rs (icon_assets! macro, AssetSource, icon()) (MIT).
// Icons: Solar Icons (Linear) by 480 Design, CC BY 4.0, as bundled by zeron.
//! Embedded SVG icons and the gpui asset source that serves them. Render with
//! `icon(icons::PLUS).size(px(16.)).text_color(…)`; gpui tints them with the text colour.

use std::borrow::Cow;

use gpui::{
    AssetSource, Div, Hsla, ParentElement as _, Result, SharedString, Styled as _, Svg, div, px,
    svg,
};

macro_rules! icon_assets {
    ($(($name:ident, $file:literal)),+ $(,)?) => {
        $(pub const $name: &str = concat!("icons/", $file, ".svg");)+

        pub struct Assets;

        impl AssetSource for Assets {
            fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
                Ok(match path {
                    $(concat!("icons/", $file, ".svg") => Some(Cow::Borrowed(
                        include_bytes!(concat!("../assets/icons/", $file, ".svg")).as_slice(),
                    )),)+
                    _ => None,
                })
            }

            fn list(&self, path: &str) -> Result<Vec<SharedString>> {
                Ok([$(concat!("icons/", $file, ".svg")),+]
                    .into_iter()
                    .filter(|p| p.starts_with(path))
                    .map(SharedString::from)
                    .collect())
            }
        }
    };
}

icon_assets!(
    (PLUS, "plus"),
    (SETTINGS, "settings"),
    (CLOSE, "close"),
    (PEN, "pen"),
    (TRASH, "trash-bin-minimalistic"),
    (COPY, "copy"),
    (PALETTE, "sun"),
    (TERMINAL, "terminal"),
    (SERVER, "remote-server"),
    (RESTART, "restart"),
    (KEYBOARD, "keyboard"),
    (CLOUD, "cloud"),
    (KEY, "key-minimalistic"),
    (INFO, "info-circle"),
    (CHEVRON_RIGHT, "alt-arrow-right"),
    (CHEVRON_LEFT, "alt-arrow-left"),
    (MORE, "more-horizontal"),
    (SEARCH, "magnifer"),
    (GLOBE, "global"),
    (FOLDER, "folder"),
    (DOCUMENT, "document"),
    (ARROW_UP, "arrow-up"),
    (HOME, "home"),
    (UPLOAD, "archive-up-minimalistic"),
);

pub fn icon(path: &'static str) -> Svg {
    svg().path(path).flex_none()
}

/// zeron's sidebar glyph: a rounded frame with a panel 5.5 wide when the sidebar is open and
/// 1.75 when closed, drawn from quads in the source's 24-unit space (gpui SVGs are static).
pub fn sidebar_glyph(open: bool, size: f32, color: Hsla) -> Div {
    let s = size / 24.0;
    let stroke = 1.75;
    let frame = div()
        .absolute()
        .left(px((3.0 - stroke / 2.0) * s))
        .top(px((4.0 - stroke / 2.0) * s))
        .w(px((18.0 + stroke) * s))
        .h(px((16.0 + stroke) * s))
        .rounded(px((4.0 + stroke / 2.0) * s))
        .border(px(stroke * s))
        .border_color(color);
    let panel = div()
        .absolute()
        .left(px(6.5 * s))
        .top(px(7.5 * s))
        .w(px(if open { 5.5 } else { 1.75 } * s))
        .h(px(9.0 * s))
        .rounded(px(0.875 * s))
        .bg(color);
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .child(frame)
        .child(panel)
}
