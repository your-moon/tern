//! Bundled Geist faces (SIL OFL 1.1, `assets/fonts/licenses`), registered once at start-up.

use std::borrow::Cow;

use gpui::App;

const FACES: [&[u8]; 7] = [
    include_bytes!("../assets/fonts/Geist.ttf"),
    include_bytes!("../assets/fonts/Geist-Medium.ttf"),
    include_bytes!("../assets/fonts/Geist-SemiBold.ttf"),
    include_bytes!("../assets/fonts/GeistMono.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Bold.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Italic.ttf"),
    include_bytes!("../assets/fonts/GeistMono-BoldItalic.ttf"),
];

/// Falls back to system fonts if registration fails; the window still opens.
pub fn register(cx: &App) {
    let faces = FACES.iter().map(|f| Cow::Borrowed(*f)).collect();
    if let Err(e) = cx.text_system().add_fonts(faces) {
        tracing::warn!(error = %e, "fonts_register_failed");
    }
}
