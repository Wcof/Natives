//! 有界 Native Messaging framing（托管应用契约 v1 §5.2）。

use crate::protocol::MAX_FRAME_BYTES;
use std::io::{self, Read, Write};

#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    TooLarge(usize),
    Truncated,
}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// 读取一帧。干净 EOF 返回 `Ok(None)`；半帧和超限均为错误，调用方必须关闭。
pub fn read_frame(reader: &mut impl Read) -> Result<Option<Vec<u8>>, FrameError> {
    let mut header = [0; 4];
    match reader.read(&mut header[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => reader
            .read_exact(&mut header[1..])
            .map_err(|error| match error.kind() {
                io::ErrorKind::UnexpectedEof => FrameError::Truncated,
                _ => FrameError::Io(error),
            })?,
        Err(error) => return Err(FrameError::Io(error)),
    }
    let length = u32::from_le_bytes(header) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(length));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|error| match error.kind() {
            io::ErrorKind::UnexpectedEof => FrameError::Truncated,
            _ => FrameError::Io(error),
        })?;
    Ok(Some(body))
}

/// 写入一帧；调用方的序列化结果同样受 512 KiB 限制。
pub fn write_frame(writer: &mut impl Write, body: &[u8]) -> Result<(), FrameError> {
    if body.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(body.len()));
    }
    writer.write_all(&(body.len() as u32).to_le_bytes())?;
    writer.write_all(body)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_clean_eof() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, b"ok").unwrap();
        let mut input = bytes.as_slice();
        assert_eq!(read_frame(&mut input).unwrap(), Some(b"ok".to_vec()));
        assert_eq!(read_frame(&mut input).unwrap(), None);
    }

    #[test]
    fn rejects_oversize_and_truncated_frames() {
        let mut oversized = ((MAX_FRAME_BYTES + 1) as u32).to_le_bytes().to_vec();
        oversized.extend_from_slice(b"x");
        assert!(matches!(
            read_frame(&mut oversized.as_slice()),
            Err(FrameError::TooLarge(_))
        ));
        assert!(matches!(
            read_frame(&mut [2, 0, 0, 0, b'x'].as_slice()),
            Err(FrameError::Truncated)
        ));
    }
}
