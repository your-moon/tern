// Adapted from russh 0.64 examples/echoserver.rs (Apache-2.0) for the server handler shape.
//! A local SSH server that accepts one password, for checking tern's login flow by hand or
//! from a script. Never expose it: it binds 127.0.0.1 only.
//!
//! `cargo run -p tern-ssh --example password_server -- 2299 hunter2`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stderr)]

use std::sync::Arc;
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use russh::server::{self, Auth, Msg, Session};
use russh::{Channel, ChannelId, MethodKind, MethodSet};

#[derive(Clone)]
struct PasswordServer(Arc<String>);

impl server::Handler for PasswordServer {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        let ok = password == self.0.as_str();
        eprintln!("auth_password user={user} accepted={ok}");
        Ok(if ok {
            Auth::Accept
        } else {
            Auth::Reject {
                proceed_with_methods: Some(MethodSet::from(&[MethodKind::Password][..])),
                partial_success: false,
            }
        })
    }

    async fn channel_open_session(
        &mut self,
        _: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
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
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().map_or(2299, |p| p.parse().expect("port"));
    let password = Arc::new(args.next().unwrap_or_else(|| "hunter2".into()));
    let config = Arc::new(server::Config {
        methods: MethodSet::from(&[MethodKind::Password][..]),
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        keys: vec![PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()],
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    eprintln!("password_server listening on 127.0.0.1:{port}");
    while let Ok((stream, _)) = listener.accept().await {
        let (config, handler) = (config.clone(), PasswordServer(password.clone()));
        tokio::spawn(async move {
            if let Ok(running) = server::run_stream(config, stream, handler).await {
                let _ = running.await;
            }
        });
    }
}
