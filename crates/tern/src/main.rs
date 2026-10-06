//! tern: a fast, low-memory SSH terminal.

mod connections;
#[cfg(debug_assertions)]
mod devkeys;
mod fonts;
mod keeper;
mod login;
mod menus;
mod pane;
mod picker;
mod runtime;
mod session;
mod settings;
mod settings_widgets;
mod shell;
mod sidebar;
mod tabs;
mod text_input;
mod theme;
mod titlebar;

use gpui::App;
use tracing_subscriber::EnvFilter;

fn main() {
    init_logging();
    log_panics();
    let _app = tracing::info_span!("app", service = "tern", env = env()).entered();
    let target = std::env::args().nth(1);
    gpui_platform::application().run(move |cx: &mut App| {
        fonts::register(cx);
        menus::init(cx);
        keeper::Keeper::install(cx);
        cx.bind_keys(tabs::bindings());
        cx.bind_keys(text_input::bindings());
        cx.bind_keys([
            gpui::KeyBinding::new("cmd-k", picker::ToggleHostPicker, None),
            gpui::KeyBinding::new("cmd-b", shell::ToggleSidebar, None),
            gpui::KeyBinding::new("cmd-n", shell::NewConnection, None),
            gpui::KeyBinding::new("cmd-,", shell::OpenSettings, None),
            gpui::KeyBinding::new("cmd-=", shell::IncreaseFontSize, None),
            gpui::KeyBinding::new("cmd-+", shell::IncreaseFontSize, None),
            gpui::KeyBinding::new("cmd--", shell::DecreaseFontSize, None),
            gpui::KeyBinding::new("cmd-0", shell::ResetFontSize, None),
        ]);
        if let Err(e) = runtime::SshRuntime::install(cx) {
            tracing::error!(error = %e, "ssh_runtime_start_failed");
            cx.quit();
            return;
        }
        let window = match shell::open_main_window(cx) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(error = %e, "window_open_failed");
                cx.quit();
                return;
            }
        };
        if let Some(target) = target {
            let _ = window.update(cx, |shell, window, cx| {
                shell.connect_target(&target, window, cx)
            });
        }
        #[cfg(debug_assertions)]
        devkeys::replay(window, cx);
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

/// A panic is also written to `~/Library/Logs/tern/panic.log` with a backtrace: launched from
/// Finder or the Dock, tern has no terminal, and a Rust panic leaves no macOS crash report.
fn log_panics() {
    let Some(dir) = std::env::home_dir().map(|h| h.join("Library/Logs/tern")) else {
        return;
    };
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let entry = format!(
            "--- {} tern {} panicked: {info}\n{backtrace}\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            env!("CARGO_PKG_VERSION"),
        );
        if std::fs::create_dir_all(&dir).is_ok() {
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("panic.log"))
            {
                let _ = f.write_all(entry.as_bytes());
            }
        }
        tracing::error!(panic = %info, "panic");
        previous(info);
    }));
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
