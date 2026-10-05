//! `cargo run -p tern-ssh --example connect -- [user@]host[:port]`
//!
//! Pipes the local terminal to an SSH session. Prompts are answered on the terminal.
//! Set `TERN_LOG=debug` for JSON tracing output on stderr.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    clippy::print_stderr
)]
// Structure follows russh russh/examples/client_exec_interactive.rs (Apache-2.0).
use std::io::{IsTerminal, Write};
use std::process::ExitCode;

use tern_ssh::{
    ConnectSpec, InputError, Prompt, SecretString, SessionEvent, SessionHandle, TermSize,
};
use tokio::io::AsyncReadExt;

struct RawMode(bool);

impl RawMode {
    fn enable() -> Self {
        RawMode(std::io::stdin().is_terminal() && crossterm::terminal::enable_raw_mode().is_ok())
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if self.0 {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}

fn term_size() -> TermSize {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    TermSize {
        cols,
        rows,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Reads a line from the terminal; `secret` disables echo.
fn read_reply(secret: bool) -> Option<String> {
    if secret {
        rpassword::read_password().ok()
    } else {
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).ok()?;
        Some(s.trim().to_string())
    }
}

async fn answer(prompt: Prompt) {
    let _ = tokio::task::spawn_blocking(move || match prompt {
        Prompt::Password { user, host, reply } => {
            eprint!("{user}@{host}'s password: ");
            let _ = reply.send(read_reply(true).map(SecretString::from));
        }
        Prompt::KeyPassphrase { path, reply } => {
            eprint!("Passphrase for {}: ", path.display());
            let _ = reply.send(read_reply(true).map(SecretString::from));
        }
        Prompt::UnknownHostKey { host, port, algorithm, fingerprint_sha256, reply } => {
            eprint!("The authenticity of {host}:{port} can't be established.\n{algorithm} key fingerprint is {fingerprint_sha256}.\nContinue connecting (yes/no)? ");
            let _ = reply.send(read_reply(false).is_some_and(|a| a == "yes"));
        }
    })
    .await;
}

fn pump_stdin(handle: SessionHandle, is_tty: bool) {
    tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let mut buf = [0u8; 4096];
        loop {
            match stdin.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    // Piped input ended: give the remote a moment to answer, then hang up.
                    if !is_tty {
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        handle.close();
                    }
                    return;
                }
                Ok(n) => {
                    if !write_all(&handle, buf[..n].to_vec()).await {
                        return;
                    }
                }
            }
        }
    });
}

/// Writes to the session, waiting while it is busy. Returns `false` once it has closed.
async fn write_all(handle: &SessionHandle, bytes: Vec<u8>) -> bool {
    loop {
        match handle.write(bytes.clone()) {
            Ok(()) => return true,
            Err(InputError::Busy) => tokio::time::sleep(std::time::Duration::from_millis(5)).await,
            Err(InputError::Closed) => return false,
        }
    }
}

#[cfg(unix)]
fn watch_resize(handle: SessionHandle) {
    tokio::spawn(async move {
        let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())
        else {
            return;
        };
        while sig.recv().await.is_some() {
            if handle.resize(term_size()) == Err(InputError::Closed) {
                return;
            }
        }
    });
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Ok(filter) = std::env::var("TERN_LOG") {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    }
    let Some(target) = std::env::args().nth(1) else {
        eprintln!("usage: connect [user@]host[:port]");
        return ExitCode::from(2);
    };
    let mut spec = match ConnectSpec::parse(&target) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("connect: {e}");
            return ExitCode::from(2);
        }
    };

    // Example-only convenience for trying host-key behaviour without touching ~/.ssh.
    spec.known_hosts = std::env::var_os("TERN_KNOWN_HOSTS").map(std::path::PathBuf::from);

    let is_tty = std::io::stdin().is_terminal();
    let (handle, events) = tern_ssh::connect(spec, term_size(), &tokio::runtime::Handle::current());
    let mut raw = None;
    let mut stdout = std::io::stdout();
    while let Ok(ev) = events.recv().await {
        match ev {
            SessionEvent::Connected => {
                raw = Some(RawMode::enable());
                #[cfg(unix)]
                if is_tty {
                    watch_resize(handle.clone());
                }
                pump_stdin(handle.clone(), is_tty);
            }
            SessionEvent::Data(d) => {
                let _ = stdout.write_all(&d);
                let _ = stdout.flush();
            }
            SessionEvent::Prompt(p) => answer(p).await,
            SessionEvent::Closed { exit_status, error } => {
                drop(raw.take());
                if let Some(e) = error {
                    eprintln!("\nconnect: {e}");
                    return ExitCode::FAILURE;
                }
                return ExitCode::from(exit_status.unwrap_or(0).min(255) as u8);
            }
        }
    }
    ExitCode::FAILURE
}
