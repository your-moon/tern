//! The server half of SOCKS5 (RFC 1928) for `DynamicForward`: no authentication, CONNECT only.
use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const VERSION: u8 = 5;
const NO_AUTH: u8 = 0;
const NO_ACCEPTABLE_METHOD: u8 = 0xFF;
const CMD_CONNECT: u8 = 1;

/// Reply codes of RFC 1928 section 6 that this side sends.
pub(crate) const REP_OK: u8 = 0;
pub(crate) const REP_GENERAL_FAILURE: u8 = 1;
pub(crate) const REP_CONNECTION_REFUSED: u8 = 5;
const REP_COMMAND_NOT_SUPPORTED: u8 = 7;
const REP_ADDRESS_NOT_SUPPORTED: u8 = 8;

/// Why a client was turned away; the reply has already been sent when this is returned.
#[derive(Debug)]
pub(crate) enum Refused {
    NotSocks5,
    Unsupported,
    Io(io::Error),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::NotSocks5 => write!(f, "not a SOCKS5 client"),
            Refused::Unsupported => write!(f, "unsupported SOCKS5 request"),
            Refused::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for Refused {
    fn from(e: io::Error) -> Self {
        Refused::Io(e)
    }
}

/// Reads the greeting and the CONNECT request; returns the destination the client asked for.
/// The caller sends the final reply with [`reply`] once it has tried the destination.
pub(crate) async fn accept<S>(s: &mut S) -> Result<(String, u16), Refused>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut head = [0u8; 2];
    s.read_exact(&mut head).await?;
    if head[0] != VERSION {
        return Err(Refused::NotSocks5);
    }
    let mut methods = vec![0u8; usize::from(head[1])];
    s.read_exact(&mut methods).await?;
    if !methods.contains(&NO_AUTH) {
        s.write_all(&[VERSION, NO_ACCEPTABLE_METHOD]).await?;
        return Err(Refused::Unsupported);
    }
    s.write_all(&[VERSION, NO_AUTH]).await?;

    let mut req = [0u8; 4];
    s.read_exact(&mut req).await?;
    if req[0] != VERSION {
        return Err(Refused::NotSocks5);
    }
    if req[1] != CMD_CONNECT {
        reply(s, REP_COMMAND_NOT_SUPPORTED).await?;
        return Err(Refused::Unsupported);
    }
    let host = match req[3] {
        1 => {
            let mut a = [0u8; 4];
            s.read_exact(&mut a).await?;
            std::net::Ipv4Addr::from(a).to_string()
        }
        4 => {
            let mut a = [0u8; 16];
            s.read_exact(&mut a).await?;
            std::net::Ipv6Addr::from(a).to_string()
        }
        3 => {
            let mut len = [0u8; 1];
            s.read_exact(&mut len).await?;
            let mut name = vec![0u8; usize::from(len[0])];
            s.read_exact(&mut name).await?;
            String::from_utf8_lossy(&name).into_owned()
        }
        _ => {
            reply(s, REP_ADDRESS_NOT_SUPPORTED).await?;
            return Err(Refused::Unsupported);
        }
    };
    let mut port = [0u8; 2];
    s.read_exact(&mut port).await?;
    Ok((host, u16::from_be_bytes(port)))
}

/// Sends the reply to a CONNECT. The bound address is not known through a tunnel, so it is
/// reported as 0.0.0.0:0, as OpenSSH does.
pub(crate) async fn reply<S: AsyncWrite + Unpin>(s: &mut S, code: u8) -> io::Result<()> {
    s.write_all(&[VERSION, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// Runs `accept` against a client that sends `bytes`; returns the result and what came back.
    async fn run(bytes: &[u8]) -> (Result<(String, u16), Refused>, Vec<u8>) {
        let (mut client, mut server) = tokio::io::duplex(1024);
        client.write_all(bytes).await.unwrap();
        let got = accept(&mut server).await;
        drop(server);
        let mut back = Vec::new();
        client.read_to_end(&mut back).await.unwrap();
        (got, back)
    }

    #[tokio::test]
    async fn connect_to_ipv4_domain_and_ipv6() {
        let (got, back) = run(&[5, 1, 0, 5, 1, 0, 1, 10, 1, 2, 3, 0x1F, 0x90]).await;
        assert_eq!(got.unwrap(), ("10.1.2.3".to_string(), 8080));
        assert_eq!(back, [5, 0]);

        let mut domain = vec![5, 2, 2, 0, 5, 1, 0, 3, 7];
        domain.extend(b"db.corp");
        domain.extend([0x15, 0x38]);
        let (got, _) = run(&domain).await;
        assert_eq!(got.unwrap(), ("db.corp".to_string(), 5432));

        let mut v6 = vec![5, 1, 0, 5, 1, 0, 4];
        v6.extend([0u8; 15]);
        v6.push(1);
        v6.extend([0, 22]);
        let (got, _) = run(&v6).await;
        assert_eq!(got.unwrap(), ("::1".to_string(), 22));
    }

    #[tokio::test]
    async fn refuses_auth_bind_and_other_versions() {
        let (got, back) = run(&[5, 1, 2]).await;
        assert!(matches!(got, Err(Refused::Unsupported)));
        assert_eq!(back, [5, 0xFF]);

        let (got, back) = run(&[5, 1, 0, 5, 2, 0, 1, 1, 1, 1, 1, 0, 80]).await;
        assert!(matches!(got, Err(Refused::Unsupported)));
        assert_eq!(&back[2..4], [5, REP_COMMAND_NOT_SUPPORTED]);

        let (got, _) = run(b"GET / HTTP/1.1\r\n").await;
        assert!(matches!(got, Err(Refused::NotSocks5)));
    }
}
