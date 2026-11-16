//! Explicit uploads to a local PicGo-compatible server. Local files stay put.
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

const MAX_RESPONSE: u64 = 1024 * 1024;
pub(crate) const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:36677/upload";

pub(crate) fn validate_endpoint(endpoint: &str) -> Result<String, String> {
    let endpoint = if endpoint.trim().is_empty() { DEFAULT_ENDPOINT } else { endpoint.trim() };
    let url = url::Url::parse(endpoint).map_err(|_| "Invalid PicGo endpoint")?;
    endpoint_parts(&url)?;
    Ok(url.into())
}

pub(crate) fn upload(path: &Path) -> Result<String, String> {
    let endpoint = std::env::var("MARKDOWNX_PICGO_ENDPOINT").ok().filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::settings::prefs().picgo_endpoint);
    let url = url::Url::parse(&endpoint).map_err(|_| "Invalid PicGo endpoint")?;
    let (address, target) = endpoint_parts(&url)?;
    let secret = std::env::var("PICGO_SERVER_SECRET").ok();
    request(address, &target, path, secret.as_deref())
}

fn endpoint_parts(url: &url::Url) -> Result<(SocketAddr, String), String> {
    if url.scheme() != "http" || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("PicGo endpoint must be a local HTTP URL".into());
    }
    let ip = match url.host_str() {
        Some("localhost" | "127.0.0.1") => IpAddr::V4(Ipv4Addr::LOCALHOST),
        Some("[::1]" | "::1") => IpAddr::V6(Ipv6Addr::LOCALHOST),
        _ => return Err("PicGo endpoint must use localhost".into()),
    };
    let address = SocketAddr::new(ip, url.port_or_known_default().ok_or("Missing PicGo port")?);
    let mut target = url.path().to_string();
    if let Some(query) = url.query() { target.push('?'); target.push_str(query); }
    Ok((address, target))
}

fn request(address: SocketAddr, target: &str, path: &Path, secret: Option<&str>) -> Result<String, String> {
    if !address.ip().is_loopback() || !target.starts_with('/') || target.contains(['\r', '\n']) { return Err("Invalid local PicGo endpoint".into()); }
    let path = path.canonicalize().map_err(|_| "Image file is unavailable")?;
    let path = path.to_str().ok_or("Image path is not valid Unicode")?;
    // Windows verbatim paths are not accepted by all PicGo versions.
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") { format!(r"\\{unc}") }
        else { path.strip_prefix(r"\\?\").unwrap_or(path).to_string() };
    let body = serde_json::to_vec(&serde_json::json!({"list": [path]})).map_err(|_| "Could not encode PicGo request")?;
    let authorization = match secret.filter(|s| !s.is_empty()) {
        Some(s) if s.bytes().all(|b| (0x20..=0x7e).contains(&b)) => format!("Authorization: Bearer {s}\r\n"),
        Some(_) => return Err("Invalid PicGo authentication secret".into()),
        None => String::new(),
    };
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|_| "Could not connect to local PicGo")?;
    socket.set_read_timeout(Some(Duration::from_secs(60))).map_err(|_| "Could not set PicGo timeout")?;
    socket.set_write_timeout(Some(Duration::from_secs(10))).map_err(|_| "Could not set PicGo timeout")?;
    write!(socket, "POST {target} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n{authorization}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()).map_err(|_| "Could not send PicGo request")?;
    socket.write_all(&body).map_err(|_| "Could not send PicGo request")?;
    let body = read_response(&mut socket)?;
    let value: serde_json::Value = serde_json::from_slice(&body).map_err(|_| "Invalid PicGo JSON response")?;
    if value.get("success").and_then(|v| v.as_bool()) != Some(true) { return Err("PicGo reported an upload failure".into()); }
    let result = value.get("result").and_then(|v| v.as_array()).and_then(|v| v.first()).and_then(|v| v.as_str()).ok_or("PicGo returned no image URL")?;
    safe_url(result)
}

/// Respect HTTP framing: local services may keep a connection open after a
/// complete response even when the client asks for Connection: close.
fn read_response(socket: &mut impl Read) -> Result<Vec<u8>, String> {
    fn read_more(socket: &mut impl Read, response: &mut Vec<u8>) -> Result<bool, String> {
        let mut buf = [0u8; 8192];
        let n = socket.read(&mut buf).map_err(|_| "Could not read PicGo response")?;
        if n == 0 { return Ok(false); }
        response.extend_from_slice(&buf[..n]);
        if response.len() as u64 > MAX_RESPONSE { return Err("PicGo response is too large".into()); }
        Ok(true)
    }
    let mut response = Vec::new();
    let boundary = loop {
        if let Some(at) = response.windows(4).position(|part| part == b"\r\n\r\n") { break at; }
        if response.len() > 32 * 1024 || !read_more(socket, &mut response)? { return Err("Invalid PicGo response".into()); }
    };
    if boundary > 32 * 1024 { return Err("Invalid PicGo response".into()); }
    let headers = std::str::from_utf8(&response[..boundary]).map_err(|_| "Invalid PicGo response")?;
    let status = headers.lines().next().and_then(|line| line.split_whitespace().nth(1)).and_then(|s| s.parse::<u16>().ok()).ok_or("Invalid PicGo status")?;
    if !(200..300).contains(&status) { return Err(format!("PicGo returned HTTP {status}")); }
    if status == 204 { return Err("PicGo returned no response body".into()); }
    let mut length = None;
    let mut chunked = false;
    for (name, value) in headers.lines().skip(1).filter_map(|line| line.split_once(':')) {
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value.trim().parse::<usize>().map_err(|_| "Invalid PicGo content length")?;
            if length.is_some_and(|old| old != parsed) { return Err("Conflicting PicGo content lengths".into()); }
            length = Some(parsed);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if !value.trim().eq_ignore_ascii_case("chunked") { return Err("Unsupported PicGo transfer encoding".into()); }
            chunked = true;
        }
    }
    if chunked && length.is_some() { return Err("Ambiguous PicGo response framing".into()); }
    let body_start = boundary + 4;
    if let Some(length) = length {
        let end = body_start.checked_add(length).ok_or("PicGo response is too large")?;
        if end as u64 > MAX_RESPONSE { return Err("PicGo response is too large".into()); }
        while response.len() < end {
            if !read_more(socket, &mut response)? { return Err("Incomplete PicGo response".into()); }
        }
        return Ok(response[body_start..end].to_vec());
    }
    if chunked {
        loop {
            if chunks_complete(&response[body_start..])? { return decode_chunks(&response[body_start..]); }
            if !read_more(socket, &mut response)? { return Err("Incomplete PicGo response".into()); }
        }
    }
    while read_more(socket, &mut response)? {}
    Ok(response[body_start..].to_vec())
}

fn chunks_complete(mut input: &[u8]) -> Result<bool, String> {
    loop {
        let Some(end) = input.windows(2).position(|b| b == b"\r\n") else { return Ok(false) };
        let size = std::str::from_utf8(&input[..end]).ok().and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or(""), 16).ok()).ok_or("Invalid PicGo chunk")?;
        input = &input[end + 2..];
        if size == 0 { return Ok(input.starts_with(b"\r\n") || input.windows(4).any(|b| b == b"\r\n\r\n")); }
        let Some(end) = size.checked_add(2) else { return Err("Invalid PicGo chunk".into()) };
        if end > input.len() { return Ok(false); }
        if &input[size..end] != b"\r\n" { return Err("Invalid PicGo chunk".into()); }
        input = &input[end..];
    }
}

fn safe_url(result: &str) -> Result<String, String> {
    let parsed = url::Url::parse(result).map_err(|_| "PicGo returned an invalid image URL")?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() { return Err("PicGo returned an unsafe image URL".into()); }
    Ok(parsed.into())
}

fn decode_chunks(mut input: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    loop {
        let end = input.windows(2).position(|b| b == b"\r\n").ok_or("Invalid PicGo chunk")?;
        let size = std::str::from_utf8(&input[..end]).ok().and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or(""), 16).ok()).ok_or("Invalid PicGo chunk")?;
        input = &input[end + 2..];
        if size == 0 { return Ok(output); }
        if size > input.len().saturating_sub(2) || &input[size..size + 2] != b"\r\n" { return Err("Invalid PicGo chunk".into()); }
        output.extend_from_slice(&input[..size]);
        input = &input[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_local_endpoints_and_defaults() {
        assert_eq!(validate_endpoint(" ").unwrap(), DEFAULT_ENDPOINT);
        assert_eq!(validate_endpoint(" http://localhost:36677/upload?config=test ").unwrap(), "http://localhost:36677/upload?config=test");
        assert!(validate_endpoint("http://[::1]:36677/upload").is_ok());
        for invalid in ["https://localhost/upload", "http://example.com/upload", "http://user:password@localhost/upload", "http://localhost/upload#fragment", "file:///tmp/image"] {
            assert!(validate_endpoint(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn reads_framed_responses_without_waiting_for_close() {
        let body = br#"{"success":true,"result":["https://example.test/image.png"]}"#;
        let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        let (sender, receiver) = std::sync::mpsc::channel();
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(&response).unwrap();
            stream.write_all(body).unwrap();
            let _ = receiver.recv_timeout(Duration::from_secs(5));
        });
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        assert_eq!(read_response(&mut stream).unwrap(), body);
        sender.send(()).unwrap();
        server.join().unwrap();
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        assert_eq!(read_response(&mut &chunked[..]).unwrap(), b"abcde");
        let truncated = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nabc";
        assert!(read_response(&mut &truncated[..]).is_err());
        let ambiguous = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert!(read_response(&mut &ambiguous[..]).is_err());
        assert!(!chunks_complete(b"3\r\nab").unwrap());
        assert!(chunks_complete(b"3\r\nabc\r\n0\r\n\r\n").unwrap());
    }

    #[test]
    fn local_upload_sends_the_path_and_checks_response() {
        let file = std::env::temp_dir().join(format!("markdownx-picgo-{}.png", uuid::Uuid::new_v4()));
        std::fs::write(&file, b"local image").unwrap();
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut data = Vec::new();
            let mut buf = [0; 2048];
            loop {
                let n = socket.read(&mut buf).unwrap();
                if n == 0 { break; }
                data.extend_from_slice(&buf[..n]);
                if let Some(end) = data.windows(4).position(|p| p == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&data[..end]);
                    let len: usize = headers.lines().find_map(|line| line.strip_prefix("Content-Length: ")).unwrap().parse().unwrap();
                    if data.len() >= end + 4 + len { break; }
                }
            }
            let request = String::from_utf8(data).unwrap();
            assert!(request.contains("Authorization: Bearer test-secret"));
            assert!(request.contains("markdownx-picgo-"));
            let body = r#"{"success":true,"result":["https://example.test/image.png"]}"#;
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        assert_eq!(request(address, "/upload", &file, Some("test-secret")).unwrap(), "https://example.test/image.png");
        server.join().unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"local image");
        assert_eq!(decode_chunks(b"3\r\nabc\r\n0\r\n\r\n").unwrap(), b"abc");
        assert!(decode_chunks(b"8\r\nabc\r\n").is_err());
        assert_eq!(safe_url("https://example.test/my image.png").unwrap(), "https://example.test/my%20image.png");
        assert!(safe_url("javascript:alert(1)").is_err());
        assert!(safe_url("https://user:secret@example.test/image.png").is_err());
        std::fs::remove_file(file).unwrap();
    }
}
