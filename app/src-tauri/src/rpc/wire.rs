//! Discord IPC wire format: 8-byte header (two little-endian u32s: opcode, payload
//! length) followed by UTF-8 JSON. See docs/dev/discord-rpc.md §1.2.

use serde_json::Value;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub mod op {
    pub const HANDSHAKE: u32 = 0;
    pub const FRAME: u32 = 1;
    pub const CLOSE: u32 = 2;
    pub const PING: u32 = 3;
    pub const PONG: u32 = 4;
}

/// Sanity cap so a corrupt length header can't allocate gigabytes.
const MAX_FRAME: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Frame {
    pub op: u32,
    pub json: Value,
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    op: u32,
    json: &Value,
) -> io::Result<()> {
    let body = serde_json::to_vec(json)?;
    let mut buf = Vec::with_capacity(8 + body.len());
    buf.extend_from_slice(&op.to_le_bytes());
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf).await
}

pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> io::Result<Frame> {
    let mut hdr = [0u8; 8];
    r.read_exact(&mut hdr).await?; // EOF here = Discord went away
    let op = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    let json =
        serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Frame { op, json })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn round_trip() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let payload = json!({"cmd": "DISPATCH", "evt": "READY", "data": {"v": 1}});
        write_frame(&mut a, op::FRAME, &payload).await.unwrap();
        let f = read_frame(&mut b).await.unwrap();
        assert_eq!(f.op, op::FRAME);
        assert_eq!(f.json, payload);
    }

    #[tokio::test]
    async fn rejects_oversized_frame() {
        let (mut a, mut b) = tokio::io::duplex(64);
        // Hand-craft a header claiming a 100 MiB body.
        let mut hdr = Vec::new();
        hdr.extend_from_slice(&op::FRAME.to_le_bytes());
        hdr.extend_from_slice(&(100u32 * 1024 * 1024).to_le_bytes());
        a.write_all(&hdr).await.unwrap();
        assert!(read_frame(&mut b).await.is_err());
    }

    #[tokio::test]
    async fn handshake_bytes_are_le() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        write_frame(&mut a, op::HANDSHAKE, &json!({"v": 1}))
            .await
            .unwrap();
        let mut raw = [0u8; 8];
        b.read_exact(&mut raw).await.unwrap();
        assert_eq!(u32::from_le_bytes(raw[0..4].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(raw[4..8].try_into().unwrap()), 7); // {"v":1}
    }
}
