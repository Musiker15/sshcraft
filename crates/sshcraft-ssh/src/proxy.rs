//! The first connection of a route: straight over TCP, or through an HTTP (CONNECT) or SOCKS5
//! proxy. Proxy credentials go only to the proxy.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zeroize::Zeroizing;

use crate::{Io, ProxyConfig, ProxyTarget, SshError};

/// The longest answer to `CONNECT` read before giving up.
const MAX_HTTP_RESPONSE: usize = 8 * 1024;

pub(crate) async fn open(
    proxy: Option<&ProxyConfig>,
    address: &str,
    port: u16,
    timeout: Duration,
) -> Result<Box<dyn Io>, SshError> {
    match proxy {
        None => {
            let tcp = tokio::time::timeout(timeout, TcpStream::connect((address, port)))
                .await
                .map_err(|_| SshError::Connect("timed out".into()))?
                .map_err(|e| SshError::Connect(e.to_string()))?;
            let _ = tcp.set_nodelay(true);
            Ok(Box::new(tcp))
        }
        Some(ProxyConfig::Http(target)) => {
            tokio::time::timeout(timeout, http(target, address, port))
                .await
                .map_err(|_| SshError::Proxy("timed out".into()))?
        }
        Some(ProxyConfig::Socks5(target)) => {
            tokio::time::timeout(timeout, socks5(target, address, port))
                .await
                .map_err(|_| SshError::Proxy("timed out".into()))?
        }
    }
}

fn authority(address: &str, port: u16) -> String {
    if address.contains(':') {
        format!("[{address}]:{port}")
    } else {
        format!("{address}:{port}")
    }
}

/// Text that goes into the proxy request: no control characters (no header can be smuggled in
/// through a synced or shared value).
fn clean(text: &str, what: &str) -> Result<(), SshError> {
    if text.chars().any(char::is_control) {
        return Err(SshError::Proxy(format!("invalid {what}")));
    }
    Ok(())
}

async fn http(proxy: &ProxyTarget, address: &str, port: u16) -> Result<Box<dyn Io>, SshError> {
    clean(address, "target address")?;
    if let Some(user) = &proxy.username {
        clean(user, "proxy user")?;
        // Basic authentication joins user and password with a colon.
        if user.contains(':') {
            return Err(SshError::Proxy("invalid proxy user".into()));
        }
    }
    if let Some(password) = &proxy.password {
        clean(password, "proxy password")?;
    }
    let mut tcp = TcpStream::connect((proxy.address.as_str(), proxy.port))
        .await
        .map_err(|e| SshError::Proxy(e.to_string()))?;
    let _ = tcp.set_nodelay(true);
    let target = authority(address, port);
    let mut request = Zeroizing::new(format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n"));
    if let Some(user) = &proxy.username {
        let password = proxy.password.as_ref().map_or("", |p| p.as_str());
        let pair = Zeroizing::new(format!("{user}:{password}"));
        let token = Zeroizing::new(data_encoding::BASE64.encode(pair.as_bytes()));
        request.push_str("Proxy-Authorization: Basic ");
        request.push_str(&token);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    tcp.write_all(request.as_bytes())
        .await
        .map_err(|e| SshError::Proxy(e.to_string()))?;
    // Byte by byte, so nothing of the SSH stream behind the answer is read away.
    let mut answer = Vec::new();
    while !answer.ends_with(b"\r\n\r\n") {
        if answer.len() >= MAX_HTTP_RESPONSE {
            return Err(SshError::Proxy("answer too long".into()));
        }
        let byte = tcp
            .read_u8()
            .await
            .map_err(|e| SshError::Proxy(e.to_string()))?;
        answer.push(byte);
    }
    let status = String::from_utf8_lossy(&answer)
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1).map(str::to_owned))
        .unwrap_or_default();
    match status.as_str() {
        "200" => Ok(Box::new(tcp)),
        "407" => Err(SshError::ProxyAuthFailed),
        "" => Err(SshError::Proxy("no HTTP answer".into())),
        code => Err(SshError::Proxy(format!("HTTP {code}"))),
    }
}

async fn socks5(proxy: &ProxyTarget, address: &str, port: u16) -> Result<Box<dyn Io>, SshError> {
    use tokio_socks::tcp::Socks5Stream;
    let server = (proxy.address.as_str(), proxy.port);
    // The name goes to the proxy as it is: the proxy resolves it.
    let target = (address, port);
    let stream = match &proxy.username {
        Some(user) => {
            let password = proxy.password.as_ref().map_or("", |p| p.as_str());
            Socks5Stream::connect_with_password(server, target, user, password).await
        }
        None => Socks5Stream::connect(server, target).await,
    }
    .map_err(|e| match e {
        tokio_socks::Error::PasswordAuthFailure(_) => SshError::ProxyAuthFailed,
        other => SshError::Proxy(other.to_string()),
    })?;
    Ok(Box::new(stream))
}
