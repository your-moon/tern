#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    clippy::print_stderr
)]
//! Drives a `TerminalView` from a LOCAL child process through `portable-pty`,
//! to exercise the view without SSH. `cargo run -p tern-term --example local_demo`.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::rc::Rc;

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use tern_term::{Terminal, TerminalTheme, TerminalView};

const COLS: u16 = 100;
const ROWS: u16 = 30;

fn main() {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = CommandBuilder::new(shell);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    let mut child = pair.slave.spawn_command(cmd).expect("spawn shell");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("reader");
    let writer = Rc::new(RefCell::new(pair.master.take_writer().expect("writer")));
    let master: Rc<RefCell<Box<dyn MasterPty + Send>>> = Rc::new(RefCell::new(pair.master));

    // Blocking reader thread -> async channel -> foreground task. The task
    // sleeps until bytes arrive; nothing polls.
    let (tx, rx) = async_channel::unbounded::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send_blocking(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    // Optional: TERN_DEMO_INIT is typed into the shell shortly after start
    // (for scripted screenshots, no synthetic keystrokes needed).
    let init_writer = writer.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        if let Ok(init) = std::env::var("TERN_DEMO_INIT") {
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(1500))
                    .await;
                let mut w = init_writer.borrow_mut();
                let _ = w.write_all(init.as_bytes());
                let _ = w.write_all(b"\r");
            })
            .detach();
        }
        let bounds = Bounds::centered(None, size(px(900.), px(560.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        let terminal_slot = Rc::new(RefCell::new(None));
        let slot = terminal_slot.clone();
        cx.open_window(options, move |window, cx| {
            let terminal = cx.new(|_| {
                Terminal::new(
                    COLS,
                    ROWS,
                    Box::new(move |bytes| {
                        let mut w = writer.borrow_mut();
                        let _ = w.write_all(&bytes);
                        let _ = w.flush();
                    }),
                    Box::new(move |cols, rows, pixel_width, pixel_height| {
                        let _ = master.borrow().resize(PtySize {
                            rows,
                            cols,
                            pixel_width,
                            pixel_height,
                        });
                    }),
                )
            });
            *slot.borrow_mut() = Some(terminal.clone());
            cx.new(|cx| TerminalView::new(terminal, TerminalTheme::default(), window, cx))
        })
        .expect("open window");

        let terminal = terminal_slot.borrow_mut().take().expect("terminal");
        cx.spawn(async move |cx| {
            while let Ok(mut bytes) = rx.recv().await {
                // Coalesce whatever else is already queued into one repaint.
                while let Ok(more) = rx.try_recv() {
                    bytes.extend(more);
                }
                terminal.update(cx, |t, cx| t.feed(&bytes, cx));
            }
            let _ = child.wait();
            cx.update(|cx| cx.quit());
        })
        .detach();
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
