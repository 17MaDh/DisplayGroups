//! Length-prefixed JSON frames.

use std::io::{Read, Write};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Upper bound for one message in either direction.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("pipe I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("message of {0} bytes exceeds the {MAX_MESSAGE_BYTES}-byte limit")]
    TooLarge(usize),
    #[error("malformed message: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error("protocol version {theirs} is not supported (this build speaks {ours})")]
    Version { theirs: u32, ours: u32 },
    #[error("{0}")]
    Rejected(String),
}

pub fn write_message<T: Serialize>(w: &mut impl Write, message: &T) -> Result<(), IpcError> {
    let body = serde_json::to_vec(message)?;
    if body.len() > MAX_MESSAGE_BYTES {
        return Err(IpcError::TooLarge(body.len()));
    }
    let len = u32::try_from(body.len()).map_err(|_| IpcError::TooLarge(body.len()))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&body)?;
    w.flush()?;
    Ok(())
}

pub fn read_message<T: DeserializeOwned>(r: &mut impl Read) -> Result<T, IpcError> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    // Checked before allocating, so a hostile length cannot exhaust memory.
    if len > MAX_MESSAGE_BYTES {
        return Err(IpcError::TooLarge(len));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Request;

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_message(&mut buf, &Request::Status).unwrap();
        write_message(&mut buf, &Request::GetAppliedWhileLocked).unwrap();
        let mut r = buf.as_slice();
        assert_eq!(read_message::<Request>(&mut r).unwrap(), Request::Status);
        assert_eq!(
            read_message::<Request>(&mut r).unwrap(),
            Request::GetAppliedWhileLocked
        );
    }

    #[test]
    fn oversized_and_garbage_frames_are_rejected() {
        let mut huge = (MAX_MESSAGE_BYTES as u32 + 1).to_le_bytes().to_vec();
        huge.extend_from_slice(b"{}");
        assert!(matches!(
            read_message::<Request>(&mut huge.as_slice()),
            Err(IpcError::TooLarge(_))
        ));

        let mut garbage = 5u32.to_le_bytes().to_vec();
        garbage.extend_from_slice(b"hello");
        assert!(matches!(
            read_message::<Request>(&mut garbage.as_slice()),
            Err(IpcError::Malformed(_))
        ));

        let truncated = 10u32.to_le_bytes();
        assert!(matches!(
            read_message::<Request>(&mut truncated.as_slice()),
            Err(IpcError::Io(_))
        ));
    }
}
