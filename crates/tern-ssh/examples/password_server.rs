// Adapted from russh 0.64 examples/echoserver.rs (Apache-2.0) for the server handler shape.
//! A local SSH server that accepts one password, for checking tern's login flow by hand or
//! from a script. Never expose it: it binds 127.0.0.1 only.
//!
//! `cargo run -p tern-ssh --example password_server -- 2299 hunter2`
//!
//! With `--sftp DIR` it also serves the `sftp` subsystem over DIR (`/` is DIR, the login
//! directory is DIR/home), the same file service tern-ssh's tests use.
//!
//! With `--otp 123456` it behaves like a 2FA server: the password is only the first step
//! (partial success), then a keyboard-interactive "Verification code:" prompt with echo off
//! must be answered with the code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stderr)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::PrivateKey;
use russh::server::{self, Auth, Msg, Session};
use russh::{Channel, ChannelId, MethodKind, MethodSet};

#[path = "../tests/common/sftp.rs"]
mod sftp;

struct PasswordServer {
    password: Arc<String>,
    otp: Option<Arc<String>>,
    /// The password step passed on this connection; the code is asked only after it.
    password_ok: bool,
    /// Served as `/` for the sftp subsystem, when asked for.
    sftp_root: Option<PathBuf>,
    /// Session channels not yet claimed by a subsystem.
    sessions: HashMap<ChannelId, Channel<Msg>>,
}

impl server::Handler for PasswordServer {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        let ok = password == self.password.as_str();
        eprintln!("auth_password user={user} accepted={ok}");
        Ok(if ok && self.otp.is_some() {
            self.password_ok = true;
            Auth::Reject {
                proceed_with_methods: Some(MethodSet::from(&[MethodKind::KeyboardInteractive][..])),
                partial_success: true,
            }
        } else if ok {
            Auth::Accept
        } else {
            Auth::Reject {
                proceed_with_methods: Some(MethodSet::from(&[MethodKind::Password][..])),
                partial_success: false,
            }
        })
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _submethods: &str,
        response: Option<server::Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        let (Some(code), true) = (self.otp.clone(), self.password_ok) else {
            return Ok(Auth::reject());
        };
        let Some(mut response) = response else {
            return Ok(Auth::Partial {
                name: "".into(),
                instructions: "".into(),
                prompts: vec![("Verification code: ".into(), false)].into(),
            });
        };
        let given = response.next().unwrap_or_default();
        let ok = given.as_ref() == code.as_bytes();
        eprintln!("auth_keyboard_interactive user={user} accepted={ok}");
        Ok(if ok { Auth::Accept } else { Auth::reject() })
    }

    /// Carries `-L` / `-D` connections: dials the destination from here and bridges it, so a
    /// forward can be checked end to end against 127.0.0.1.
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        let dest = (host.to_owned(), port as u16);
        match tokio::net::TcpStream::connect(dest).await {
            Ok(mut tcp) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
                });
            }
            Err(e) => eprintln!("direct-tcpip {host}:{port} failed: {e}"),
        }
        Ok(())
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.sessions.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        ch: ChannelId,
        name: &str,
        s: &mut Session,
    ) -> Result<(), Self::Error> {
        match (name, self.sftp_root.clone(), self.sessions.remove(&ch)) {
            ("sftp", Some(root), Some(channel)) => {
                s.channel_success(ch)?;
                tokio::spawn(russh_sftp::server::run(
                    channel.into_stream(),
                    sftp::FsHandler::new(root),
                ));
            }
            _ => s.channel_failure(ch)?,
        }
        Ok(())
    }

    async fn shell_request(&mut self, ch: ChannelId, s: &mut Session) -> Result<(), Self::Error> {
        s.channel_success(ch)?;
        s.data(ch, &b"password_server: logged in\r\n"[..])?;
        Ok(())
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let otp = args.iter().position(|a| a == "--otp").map(|i| {
        let code = args.get(i + 1).cloned().expect("--otp needs a code");
        args.drain(i..=i + 1);
        Arc::new(code)
    });
    let sftp_root = args.iter().position(|a| a == "--sftp").map(|i| {
        let dir = PathBuf::from(args.get(i + 1).cloned().expect("--sftp needs a directory"));
        args.drain(i..=i + 1);
        std::fs::create_dir_all(dir.join("home")).expect("create the sftp home");
        dir
    });
    let mut args = args.into_iter();
    let port: u16 = args.next().map_or(2299, |p| p.parse().expect("port"));
    let password = Arc::new(args.next().unwrap_or_else(|| "hunter2".into()));
    let config = Arc::new(server::Config {
        // Only the password is offered at first; the code is offered after it, as sshd's
        // `AuthenticationMethods password,keyboard-interactive` does.
        methods: MethodSet::from(&[MethodKind::Password][..]),
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        // A fixed key, so a restarted server is the same host to known_hosts (reconnect checks).
        keys: vec![PrivateKey::from(
            russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[7; 32]),
        )],
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    eprintln!("password_server listening on 127.0.0.1:{port}");
    while let Ok((stream, _)) = listener.accept().await {
        let handler = PasswordServer {
            password: password.clone(),
            otp: otp.clone(),
            password_ok: false,
            sftp_root: sftp_root.clone(),
            sessions: HashMap::new(),
        };
        let config = config.clone();
        tokio::spawn(async move {
            if let Ok(running) = server::run_stream(config, stream, handler).await {
                let _ = running.await;
            }
        });
    }
}
