//! tern: a fast, low-memory SSH terminal.

mod fonts;
mod shell;
mod theme;

use gpui::App;
use tracing_subscriber::EnvFilter;

fn main() {
    init_logging();
    let _app = tracing::info_span!("app", service = "tern", env = env()).entered();
    gpui_platform::application().run(|cx: &mut App| {
        fonts::register(cx);
        if let Err(e) = shell::open_main_window(cx) {
            tracing::error!(error = %e, "window_open_failed");
            cx.quit();
            return;
        }
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
        tracing::info!("app_started");
    });
}

/// Structured JSON logs on stderr, filtered by `TERN_LOG` (default `info`).
fn init_logging() {
    tracing_subscriber::fmt()
        .json()
        .with_current_span(true)
        .with_env_filter(EnvFilter::try_from_env("TERN_LOG").unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .init();
}

fn env() -> &'static str {
    if cfg!(debug_assertions) {
        "dev"
    } else {
        "production"
    }
}
