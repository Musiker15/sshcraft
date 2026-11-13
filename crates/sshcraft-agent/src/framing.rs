//! One JSON object per line, at most [`MAX_LINE`] bytes, so a peer cannot make the other side
//! buffer without end.

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_LINE: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("line longer than {MAX_LINE} bytes")]
    TooLong,
    #[error("not a valid message: {0}")]
    Malformed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// The next message, or `None` at the end of the stream.
pub async fn read_message<T: DeserializeOwned, R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<T>, FrameError> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            break;
        }
        let (take, done) = match available.iter().position(|&b| b == b'\n') {
            Some(at) => (at + 1, true),
            None => (available.len(), false),
        };
        if line.len() + take > MAX_LINE + 1 {
            return Err(FrameError::TooLong);
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if done {
            break;
        }
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    if line.len() > MAX_LINE {
        return Err(FrameError::TooLong);
    }
    serde_json::from_slice(&line)
        .map(Some)
        .map_err(|e| FrameError::Malformed(e.to_string()))
}

pub async fn write_message<T: Serialize, W: AsyncWrite + Unpin>(
    writer: &mut W,
    message: &T,
) -> Result<(), FrameError> {
    let mut line = serde_json::to_vec(message).map_err(|e| FrameError::Malformed(e.to_string()))?;
    if line.len() > MAX_LINE {
        return Err(FrameError::TooLong);
    }
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn messages_come_back_line_by_line() {
        let mut buf = Vec::new();
        write_message(&mut buf, &serde_json::json!({"a": 1}))
            .await
            .unwrap();
        write_message(&mut buf, &serde_json::json!({"b": "x\ny"}))
            .await
            .unwrap();
        let mut reader = BufReader::new(buf.as_slice());
        let one: serde_json::Value = read_message(&mut reader).await.unwrap().unwrap();
        let two: serde_json::Value = read_message(&mut reader).await.unwrap().unwrap();
        assert_eq!(one["a"], 1);
        assert_eq!(two["b"], "x\ny");
        assert!(
            read_message::<serde_json::Value, _>(&mut reader)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_endless_line_is_refused_early() {
        // A reader that never ends a line: refused once it passes the limit.
        let endless = tokio::io::repeat(b'a');
        let mut reader = BufReader::with_capacity(64 * 1024, endless);
        let result = read_message::<serde_json::Value, _>(&mut reader).await;
        assert!(matches!(result, Err(FrameError::TooLong)));
    }

    #[tokio::test]
    async fn garbage_is_malformed() {
        let mut reader = BufReader::new(&b"not json\n"[..]);
        assert!(matches!(
            read_message::<serde_json::Value, _>(&mut reader).await,
            Err(FrameError::Malformed(_))
        ));
    }
}
