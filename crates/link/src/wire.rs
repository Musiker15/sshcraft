//! One JSON object per line, in both directions, each line bounded.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// A request: `params` may be left out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// The answer to request `id`: a `result` or an `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(id: u64, result: Value) -> Self {
        Self { id, ok: true, result: Some(result), error: None }
    }

    pub fn error(id: u64, error: impl Into<String>) -> Self {
        Self { id, ok: false, result: None, error: Some(error.into()) }
    }
}

/// What reading a line gave.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    Text(Vec<u8>),
    /// The other end closed the connection.
    End,
    /// The line passed `max` bytes before its end.
    TooLong,
}

/// Reads one line of at most `max` bytes (without its newline).
pub(crate) async fn read_line<R: AsyncBufRead + Unpin>(read: &mut R, max: usize) -> std::io::Result<Line> {
    let mut buf = Vec::new();
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let n = (&mut *read).take(limit).read_until(b'\n', &mut buf).await?;
    if n == 0 {
        return Ok(Line::End);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        return Ok(Line::Text(buf));
    }
    if buf.len() > max {
        return Ok(Line::TooLong);
    }
    // The last line without a newline before the end.
    Ok(Line::Text(buf))
}

/// Writes `value` as one line; `Err` with `InvalidData` when it would pass `max` bytes.
pub(crate) async fn write_line<W: AsyncWrite + Unpin, T: Serialize>(write: &mut W, value: &T, max: usize) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if bytes.len() >= max {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "line too long"));
    }
    bytes.push(b'\n');
    write.write_all(&bytes).await?;
    write.flush().await
}
