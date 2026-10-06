// Adapted from zeron's local terminal (portable-pty session) (MIT).
//! A shell on this machine in a pseudo-terminal. It speaks the same [`SessionEvent`]s as an
//! SSH session, so the terminal view and the tab code do not care which one they hold.

use std::io::{Read, Write};
use std::sync::mpsc;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tern_ssh::{SessionEvent, TermSize};

/// Output read from the pty at a time.
const READ_CHUNK: usize = 16 * 1024;

/// The login shell: `$SHELL`, or zsh (the macOS default) when it is unset or empty.
pub fn login_shell() -> CommandBuilder {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".into());
    let mut cmd = CommandBuilder::new(shell);
    cmd.arg("-l");
    if let Some(home) = std::env::home_dir() {
        cmd.cwd(home);
    }
    cmd
}

/// A running local shell. Dropping it kills the shell, as closing a tab should.
pub struct LocalPty {
    master: Box<dyn MasterPty + Send>,
    input: mpsc::Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

impl std::fmt::Debug for LocalPty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPty").finish_non_exhaustive()
    }
}

impl LocalPty {
    /// Starts `cmd` in a new pty. The events end with exactly one [`SessionEvent::Closed`].
    ///
    /// # Errors
    ///
    /// The reason the pty or the process could not be started.
    pub fn spawn(
        mut cmd: CommandBuilder,
        size: TermSize,
    ) -> Result<(Self, async_channel::Receiver<SessionEvent>), String> {
        let pair = native_pty_system()
            .openpty(pty_size(size))
            .map_err(|e| e.to_string())?;
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        // The child holds its end now; keeping ours would stop the reader seeing the end.
        drop(pair.slave);
        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let (etx, erx) = async_channel::bounded(64);
        // Cannot fail: the channel is new and has room.
        let _ = etx.try_send(SessionEvent::Connected);
        std::thread::spawn(move || {
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if etx
                            .send_blocking(SessionEvent::Data(buf[..n].to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            let exit_status = child.wait().ok().map(|s| s.exit_code());
            let _ = etx.send_blocking(SessionEvent::Closed {
                exit_status,
                error: None,
            });
        });

        let (input, queue) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            while let Ok(bytes) = queue.recv() {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });

        Ok((
            Self {
                master: pair.master,
                input,
                killer,
            },
            erx,
        ))
    }

    /// Queues keystrokes for the shell; false once it has gone.
    pub fn write(&self, bytes: Vec<u8>) -> bool {
        self.input.send(bytes).is_ok()
    }

    pub fn resize(&self, size: TermSize) {
        if let Err(e) = self.master.resize(pty_size(size)) {
            tracing::debug!(error = %e, "local_resize_skipped");
        }
    }
}

impl Drop for LocalPty {
    fn drop(&mut self) {
        // Already exited is fine.
        let _ = self.killer.kill();
    }
}

fn pty_size(size: TermSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: size.pixel_width,
        pixel_height: size.pixel_height,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const SIZE: TermSize = TermSize {
        cols: 80,
        rows: 24,
        pixel_width: 0,
        pixel_height: 0,
    };

    /// Reads events until the shell closes: all the output, and the exit status.
    fn run(script: &str) -> (String, Option<u32>) {
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg(script);
        let (_pty, events) = LocalPty::spawn(cmd, SIZE).unwrap();
        let mut out = Vec::new();
        loop {
            match events.recv_blocking().unwrap() {
                SessionEvent::Data(bytes) => out.extend(bytes),
                SessionEvent::Closed { exit_status, .. } => {
                    return (String::from_utf8_lossy(&out).into_owned(), exit_status);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn output_and_a_clean_exit_arrive_as_events() {
        let (out, status) = run("printf hello-pty");
        assert!(out.contains("hello-pty"), "{out:?}");
        assert_eq!(status, Some(0));
    }

    #[test]
    fn a_failing_shell_reports_its_status() {
        let (_, status) = run("exit 7");
        assert_eq!(status, Some(7));
    }

    #[test]
    fn input_reaches_the_shell_and_the_size_is_the_one_asked_for() {
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg("stty size; read line; echo got:$line");
        let size = TermSize {
            cols: 100,
            rows: 30,
            ..SIZE
        };
        let (pty, events) = LocalPty::spawn(cmd, size).unwrap();
        assert!(pty.write(b"abc\n".to_vec()));
        let mut out = String::new();
        while let Ok(event) = events.recv_blocking() {
            match event {
                SessionEvent::Data(bytes) => out.push_str(&String::from_utf8_lossy(&bytes)),
                SessionEvent::Closed { .. } => break,
                _ => {}
            }
        }
        assert!(out.contains("30 100"), "{out:?}");
        assert!(out.contains("got:abc"), "{out:?}");
    }
}
