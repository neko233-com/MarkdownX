//! Bounded LSP-style Content-Length framing for extension JSON-RPC streams.
//! This module does not spawn or trust extensions; callers own process lifetime.
use std::io::{BufRead, Write};
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;

pub fn write_message(writer: &mut impl Write, message: &Value) -> Result<()> {
    ensure!(message.is_object(), "JSON-RPC message must be an object");
    let payload = serde_json::to_vec(message)?;
    ensure!(payload.len() <= MAX_MESSAGE_BYTES, "extension message is too large");
    write!(writer, "Content-Length: {}\r\n\r\n", payload.len())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Read exactly one framed message without consuming bytes from the next one.
pub fn read_message(reader: &mut impl BufRead) -> Result<Value> {
    let mut headers = 0usize;
    let mut length = None;
    loop {
        let mut line = Vec::new();
        loop {
            let chunk = reader.fill_buf()?;
            ensure!(!chunk.is_empty(), "unexpected end of extension headers");
            let count = chunk.iter().position(|byte| *byte == b'\n').map_or(chunk.len(), |index| index + 1);
            headers = headers.checked_add(count).ok_or_else(|| anyhow::anyhow!("extension headers are too large"))?;
            ensure!(headers <= MAX_HEADER_BYTES, "extension headers are too large");
            line.extend_from_slice(&chunk[..count]);
            reader.consume(count);
            if line.last() == Some(&b'\n') { break; }
        }
        ensure!(line.ends_with(b"\r\n"), "invalid extension header line ending");
        line.truncate(line.len() - 2);
        if line.is_empty() { break; }
        let line = std::str::from_utf8(&line).context("invalid extension header encoding")?;
        let (name, value) = line.split_once(':').ok_or_else(|| anyhow::anyhow!("invalid extension header"))?;
        ensure!(!name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'), "invalid extension header name");
        if name.eq_ignore_ascii_case("content-length") {
            ensure!(length.is_none(), "duplicate extension content length");
            let value = value.trim();
            ensure!(!value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()), "invalid extension content length");
            let parsed = value.parse::<usize>().context("invalid extension content length")?;
            ensure!(parsed <= MAX_MESSAGE_BYTES, "extension message is too large");
            length = Some(parsed);
        }
    }
    let Some(length) = length else { bail!("missing extension content length") };
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).context("incomplete extension message")?;
    let value: Value = serde_json::from_slice(&payload).context("invalid extension JSON")?;
    ensure!(value.is_object(), "JSON-RPC message must be an object");
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};
    use serde_json::json;

    #[test]
    fn round_trips_unicode_and_consecutive_frames_with_fragmented_reads() {
        let a = json!({"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"uri":"file:///测试.md"}});
        let b = json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}});
        let mut wire = Vec::new();
        write_message(&mut wire, &a).unwrap();
        write_message(&mut wire, &b).unwrap();
        let mut reader = BufReader::with_capacity(3, Cursor::new(wire));
        assert_eq!(read_message(&mut reader).unwrap(), a);
        assert_eq!(read_message(&mut reader).unwrap(), b);
        assert!(read_message(&mut reader).is_err());
    }

    #[test]
    fn accepts_case_insensitive_headers_whitespace_and_extra_fields() {
        for bytes in [
            &b"content-length: 2\r\n\r\n{}"[..],
            &b"Content-Length:  2 \r\n\r\n{}"[..],
            &b"X-Extension: value\r\nCONTENT-LENGTH: 2\r\n\r\n{}"[..],
        ] {
            assert_eq!(read_message(&mut BufReader::new(Cursor::new(bytes))).unwrap(), json!({}));
        }
    }

    #[test]
    fn rejects_missing_zero_and_malformed_lengths() {
        for bytes in [
            &b"\r\n{}"[..],
            &b"Content-Length: 0\r\n\r\n"[..],
            &b"Content-Length: +2\r\n\r\n{}"[..],
            &b"Content-Length: 2.0\r\n\r\n{}"[..],
            &b"Bad Name: value\r\nContent-Length: 2\r\n\r\n{}"[..],
            &b"Content-Length: 2\r\n\r\n"[..],
        ] { assert!(read_message(&mut BufReader::new(Cursor::new(bytes))).is_err(), "{:?}", String::from_utf8_lossy(bytes)); }
    }

    #[test]
    fn rejects_ambiguous_truncated_and_oversized_frames() {
        for bytes in [
            &b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}"[..],
            &b"Content-Length: -1\r\n\r\n"[..],
            &b"Content-Length: 16777217\r\n\r\n"[..],
            &b"Content-Length: 2\n\n{}"[..],
            &b"Content-Length: 2\r\n\r\n{"[..],
            &b"Content-Length: 2\r\n\r\n[]"[..],
            &b"Other: value\r\n\r\n{}"[..],
        ] { assert!(read_message(&mut BufReader::new(Cursor::new(bytes))).is_err()); }
        let long = vec![b'x'; MAX_HEADER_BYTES + 1];
        assert!(read_message(&mut BufReader::new(Cursor::new(long))).is_err());
        assert!(write_message(&mut Vec::new(), &json!([])).is_err());
    }
}
