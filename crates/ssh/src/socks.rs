//! The server side of SOCKS5 (RFC 1928) for dynamic forwarding: no authentication, `CONNECT`
//! only. Anything else gets the matching SOCKS error and the connection ends.

use std::net::SocketAddr;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const VERSION: u8 = 5;
const NO_AUTH: u8 = 0x00;
const NO_ACCEPTABLE: u8 = 0xFF;
const CONNECT: u8 = 0x01;
pub const GENERAL_FAILURE: u8 = 0x01;
pub const HOST_UNREACHABLE: u8 = 0x04;
const COMMAND_NOT_SUPPORTED: u8 = 0x07;
const ADDRESS_NOT_SUPPORTED: u8 = 0x08;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SocksError {
    #[error("socks: {0}")]
    Io(String),
    #[error("socks: not version 5")]
    Version,
    #[error("socks: the client offers no method without authentication")]
    NoAcceptableMethod,
    #[error("socks: command {0} is not supported")]
    Command(u8),
    #[error("socks: address type not supported")]
    AddressType,
}

/// Reads the greeting and the request; returns the target the client wants.
pub async fn read_request<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
) -> Result<(String, u16), SocksError> {
    let [version, count] = read_array::<2, _>(stream).await?;
    if version != VERSION {
        return Err(SocksError::Version);
    }
    let mut methods = vec![0u8; usize::from(count)];
    stream.read_exact(&mut methods).await.map_err(io)?;
    if !methods.contains(&NO_AUTH) {
        write(stream, &[VERSION, NO_ACCEPTABLE]).await?;
        return Err(SocksError::NoAcceptableMethod);
    }
    write(stream, &[VERSION, NO_AUTH]).await?;
    let [version, command, _reserved, address_type] = read_array::<4, _>(stream).await?;
    if version != VERSION {
        return Err(SocksError::Version);
    }
    if command != CONNECT {
        reply_failure(stream, COMMAND_NOT_SUPPORTED).await?;
        return Err(SocksError::Command(command));
    }
    let host = match address_type {
        1 => std::net::Ipv4Addr::from(read_array::<4, _>(stream).await?).to_string(),
        4 => std::net::Ipv6Addr::from(read_array::<16, _>(stream).await?).to_string(),
        3 => {
            let [len] = read_array::<1, _>(stream).await?;
            let mut name = vec![0u8; usize::from(len)];
            stream.read_exact(&mut name).await.map_err(io)?;
            match String::from_utf8(name) {
                Ok(name) if !name.is_empty() => name,
                _ => {
                    reply_failure(stream, GENERAL_FAILURE).await?;
                    return Err(SocksError::AddressType);
                }
            }
        }
        _ => {
            reply_failure(stream, ADDRESS_NOT_SUPPORTED).await?;
            return Err(SocksError::AddressType);
        }
    };
    let port = u16::from_be_bytes(read_array::<2, _>(stream).await?);
    Ok((host, port))
}

pub async fn reply_success<S: AsyncWrite + Unpin>(
    stream: &mut S,
    bound: SocketAddr,
) -> Result<(), SocksError> {
    let mut reply = vec![VERSION, 0, 0];
    match bound {
        SocketAddr::V4(a) => {
            reply.push(1);
            reply.extend_from_slice(&a.ip().octets());
        }
        SocketAddr::V6(a) => {
            reply.push(4);
            reply.extend_from_slice(&a.ip().octets());
        }
    }
    reply.extend_from_slice(&bound.port().to_be_bytes());
    write(stream, &reply).await
}

pub async fn reply_failure<S: AsyncWrite + Unpin>(
    stream: &mut S,
    code: u8,
) -> Result<(), SocksError> {
    write(stream, &[VERSION, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}

fn io(e: std::io::Error) -> SocksError {
    SocksError::Io(e.to_string())
}

async fn read_array<const N: usize, S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<[u8; N], SocksError> {
    let mut buf = [0u8; N];
    stream.read_exact(&mut buf).await.map_err(io)?;
    Ok(buf)
}

async fn write<S: AsyncWrite + Unpin>(stream: &mut S, bytes: &[u8]) -> Result<(), SocksError> {
    stream.write_all(bytes).await.map_err(io)?;
    stream.flush().await.map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    /// Runs `read_request` against a client that sends `sent` and returns the result plus every
    /// byte the server answered.
    async fn talk(sent: &[u8]) -> (Result<(String, u16), SocksError>, Vec<u8>) {
        let (mut client, mut server) = duplex(1024);
        client.write_all(sent).await.unwrap();
        // The client sends nothing more: a short request ends instead of waiting.
        client.shutdown().await.unwrap();
        let result = read_request(&mut server).await;
        drop(server);
        let mut answer = Vec::new();
        client.read_to_end(&mut answer).await.unwrap();
        (result, answer)
    }

    #[tokio::test]
    async fn ipv4_target() {
        let (result, answer) = talk(&[5, 1, 0, 5, 1, 0, 1, 10, 0, 0, 7, 0x1F, 0x90]).await;
        assert_eq!(result, Ok(("10.0.0.7".to_owned(), 8080)));
        assert_eq!(answer, vec![5, 0]);
    }

    #[tokio::test]
    async fn ipv6_target() {
        let mut sent = vec![5, 1, 0, 5, 1, 0, 4];
        sent.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        sent.extend_from_slice(&[0, 22]);
        let (result, _) = talk(&sent).await;
        assert_eq!(result, Ok(("::1".to_owned(), 22)));
    }

    #[tokio::test]
    async fn name_target() {
        let mut sent = vec![5, 2, 2, 0, 5, 1, 0, 3, 11];
        sent.extend_from_slice(b"example.org");
        sent.extend_from_slice(&[1, 187]);
        let (result, answer) = talk(&sent).await;
        assert_eq!(result, Ok(("example.org".to_owned(), 443)));
        assert_eq!(answer, vec![5, 0]);
    }

    #[tokio::test]
    async fn socks4_is_refused() {
        let (result, _) = talk(&[4, 1, 0, 80, 10, 0, 0, 1, 0]).await;
        assert_eq!(result, Err(SocksError::Version));
    }

    #[tokio::test]
    async fn only_password_methods_are_refused() {
        let (result, answer) = talk(&[5, 1, 2]).await;
        assert_eq!(result, Err(SocksError::NoAcceptableMethod));
        assert_eq!(answer, vec![5, 0xFF]);
    }

    #[tokio::test]
    async fn bind_and_udp_are_refused_with_code_7() {
        for command in [2u8, 3] {
            let (result, answer) = talk(&[5, 1, 0, 5, command, 0, 1, 10, 0, 0, 7, 0, 80]).await;
            assert_eq!(result, Err(SocksError::Command(command)));
            assert_eq!(answer[..2], [5, 0]);
            assert_eq!(answer[2..4], [5, 7]);
        }
    }

    #[tokio::test]
    async fn empty_name_and_unknown_address_type_are_refused() {
        let (result, answer) = talk(&[5, 1, 0, 5, 1, 0, 3, 0, 0, 80]).await;
        assert_eq!(result, Err(SocksError::AddressType));
        assert_eq!(answer[2..4], [5, 1]);
        let (result, answer) = talk(&[5, 1, 0, 5, 1, 0, 9, 0, 80]).await;
        assert_eq!(result, Err(SocksError::AddressType));
        assert_eq!(answer[2..4], [5, 8]);
    }

    #[tokio::test]
    async fn a_cut_off_request_ends_with_an_error() {
        let (result, _) = talk(&[5, 1, 0, 5, 1, 0, 1, 10, 0]).await;
        assert!(matches!(result, Err(SocksError::Io(_))));
        let (result, _) = talk(&[5]).await;
        assert!(matches!(result, Err(SocksError::Io(_))));
    }

    #[tokio::test]
    async fn replies_have_the_socks5_layout() {
        let mut out = Vec::new();
        reply_success(&mut out, "127.0.0.1:1080".parse().unwrap())
            .await
            .unwrap();
        assert_eq!(out, vec![5, 0, 0, 1, 127, 0, 0, 1, 0x04, 0x38]);
        let mut out = Vec::new();
        reply_failure(&mut out, HOST_UNREACHABLE).await.unwrap();
        assert_eq!(out, vec![5, 4, 0, 1, 0, 0, 0, 0, 0, 0]);
    }
}
