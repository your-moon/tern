//! The one tokio runtime all SSH sessions share, on its own thread, so network work never
//! runs on the UI thread and there is no thread per connection.

use gpui::{App, Global};

pub struct SshRuntime(tokio::runtime::Runtime);

impl Global for SshRuntime {}

impl SshRuntime {
    pub fn install(cx: &mut App) -> std::io::Result<()> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("tern-ssh")
            .enable_all()
            .build()?;
        cx.set_global(Self(rt));
        Ok(())
    }

    pub fn handle(cx: &App) -> tokio::runtime::Handle {
        cx.global::<Self>().0.handle().clone()
    }
}
