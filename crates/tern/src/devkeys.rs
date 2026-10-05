//! Debug builds only: `TERN_DEV_KEYS="cmd-k s t b"` replays keystrokes into the window after
//! start-up, so a scripted visual check can drive the app without taking keyboard focus from
//! whatever window the person at the machine is using.

use std::time::Duration;

use gpui::{App, AppContext, Keystroke, WindowHandle};

use crate::shell::Shell;

pub fn replay(window: WindowHandle<Shell>, cx: &mut App) {
    let Ok(script) = std::env::var("TERN_DEV_KEYS") else {
        return;
    };
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(1500))
            .await;
        for source in script.split_whitespace() {
            let Ok(keystroke) = Keystroke::parse(source) else {
                tracing::warn!(key = source, "dev_key_unparsed");
                continue;
            };
            let handled = cx.update_window(window.into(), |_, window, cx| {
                window.dispatch_keystroke(keystroke, cx)
            });
            tracing::debug!(key = source, ?handled, "dev_key");
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
        }
    })
    .detach();
}
