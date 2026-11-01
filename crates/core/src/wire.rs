//! Byte-level framing shared by TCP and QUIC streams.
//!
//! Control frame: `u32 LE length` + JSON.
//! Chunk frame:   `0xC1, item u32, index u32, raw_len u32, flags u8, wire_len u32`,
//!                then `wire_len` bytes, then the 32-byte BLAKE3 of the raw bytes.
//! End frame:     `0xE0` (the sender has nothing more for this stream).

use anyhow::{bail, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::proto::Ctrl;

pub const TAG_CHUNK: u8 = 0xC1;
pub const TAG_END: u8 = 0xE0;
pub const FLAG_ZSTD: u8 = 1;
pub const CHUNK_HEADER_LEN: usize = 18;
const MAX_CTRL: usize = 64 << 20;

pub async fn write_ctrl<W: AsyncWrite + Unpin + ?Sized>(w: &mut W, msg: &Ctrl) -> Result<()> {
    let body = serde_json::to_vec(msg)?;
    let mut buf = Vec::with_capacity(body.len() + 4);
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_ctrl<R: AsyncRead + Unpin + ?Sized>(r: &mut R) -> Result<Ctrl> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_CTRL {
        bail!("control frame too large ({len} bytes)");
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkHeader {
    pub item: u32,
    pub index: u32,
    pub raw_len: u32,
    pub flags: u8,
    pub wire_len: u32,
}

impl ChunkHeader {
    pub fn encode(&self) -> [u8; CHUNK_HEADER_LEN] {
        let mut b = [0u8; CHUNK_HEADER_LEN];
        b[0] = TAG_CHUNK;
        b[1..5].copy_from_slice(&self.item.to_le_bytes());
        b[5..9].copy_from_slice(&self.index.to_le_bytes());
        b[9..13].copy_from_slice(&self.raw_len.to_le_bytes());
        b[13] = self.flags;
        b[14..18].copy_from_slice(&self.wire_len.to_le_bytes());
        b
    }
    fn decode(b: &[u8; CHUNK_HEADER_LEN - 1]) -> Self {
        let u = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        Self { item: u(0), index: u(4), raw_len: u(8), flags: b[12], wire_len: u(13) }
    }
}

/// `None` = end of stream.
pub async fn read_chunk_header<R: AsyncRead + Unpin + ?Sized>(r: &mut R) -> Result<Option<ChunkHeader>> {
    let tag = r.read_u8().await?;
    match tag {
        TAG_END => Ok(None),
        TAG_CHUNK => {
            let mut b = [0u8; CHUNK_HEADER_LEN - 1];
            r.read_exact(&mut b).await?;
            Ok(Some(ChunkHeader::decode(&b)))
        }
        t => bail!("unexpected frame tag 0x{t:02x}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_roundtrip() {
        let h = ChunkHeader { item: 7, index: 300, raw_len: 4 << 20, flags: FLAG_ZSTD, wire_len: 1234 };
        let mut buf = Vec::new();
        buf.extend_from_slice(&h.encode());
        buf.push(TAG_END);
        write_ctrl(&mut buf, &Ctrl::Pause).await.unwrap();
        let mut r = &buf[..];
        assert_eq!(read_chunk_header(&mut r).await.unwrap(), Some(h));
        assert_eq!(read_chunk_header(&mut r).await.unwrap(), None);
        assert!(matches!(read_ctrl(&mut r).await.unwrap(), Ctrl::Pause));
    }
}
