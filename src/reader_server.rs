//! On-demand, loopback-only static reader. It serves only assets embedded in the app.
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};
use std::time::Duration;

struct ReaderServer { address: SocketAddr, token: String, stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>> }

impl ReaderServer {
    fn start() -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let address = listener.local_addr()?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let prefix = format!("/{token}/");
        let stop = Arc::new(AtomicBool::new(false));
        let ending = stop.clone();
        let thread = thread::Builder::new().name("markdownx-offline-reader".into()).spawn(move || {
            for connection in listener.incoming() {
                if ending.load(Ordering::Acquire) { break; }
                if let Ok(mut stream) = connection { let _ = respond(&mut stream, &prefix); }
            }
        })?;
        Ok(Self { address, token, stop, thread: Some(thread) })
    }

    fn url(&self) -> String { format!("http://{}/{}/reader/", self.address, self.token) }
}

impl Drop for ReaderServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(100));
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}

/// Started only after the user requests the reader; no collaboration service is needed.
pub(crate) fn url() -> Result<String, String> {
    static SERVER: OnceLock<Mutex<Option<ReaderServer>>> = OnceLock::new();
    let mut server = SERVER.get_or_init(|| Mutex::new(None)).lock().map_err(|e| e.to_string())?;
    if server.is_none() { *server = Some(ReaderServer::start().map_err(|e| e.to_string())?); }
    server.as_ref().map(ReaderServer::url).ok_or_else(|| "reader did not start".into())
}

fn respond(stream: &mut TcpStream, prefix: &str) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut request = [0u8; 8192];
    let mut used = 0;
    while used < request.len() {
        let count = stream.read(&mut request[used..])?;
        if count == 0 { return Ok(()); }
        used += count;
        if request[..used].windows(4).any(|w| w == b"\r\n\r\n") { break; }
    }
    let line = std::str::from_utf8(&request[..used]).ok().and_then(|s| s.lines().next()).unwrap_or("");
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("").split('?').next().unwrap_or("");
    let found = (method == "GET").then(|| path.strip_prefix(prefix)).flatten().and_then(asset);
    let (status, mime, body) = found.map(|(mime, body)| ("200 OK", mime, body)).unwrap_or(("404 Not Found", "text/plain; charset=utf-8", b"Not found"));
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(body)
}

fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    const HTML: &str = "text/html; charset=utf-8";
    const CSS: &str = "text/css; charset=utf-8";
    const JS: &str = "text/javascript; charset=utf-8";
    const JSON: &str = "application/json; charset=utf-8";
    const BOOK: &str = "application/octet-stream";
    Some(match path {
        "reader/" | "reader/index.html" => (HTML, include_bytes!("../web/index.html")),
        "reader/style.css" => (CSS, include_bytes!("../web/style.css")),
        "reader/js/app.js" => (JS, include_bytes!("../web/js/app.js")),
        "reader/js/theme.js" => (JS, include_bytes!("../web/js/theme.js")),
        "reader/js/docxml.js" => (JS, include_bytes!("../web/js/docxml.js")),
        "reader/js/katex.mjs" => (JS, include_bytes!("../web/js/katex.mjs")),
        "reader/js/markdown.js" => (JS, include_bytes!("../web/js/markdown.js")),
        "reader/js/mdxb.js" => (JS, include_bytes!("../web/js/mdxb.js")),
        "reader/js/widgets.js" => (JS, include_bytes!("../web/js/widgets.js")),
        "reader/js/model3d.js" => (JS, include_bytes!("../web/js/model3d.js")),
        "reader/lib/three/three.module.js" => (JS, include_bytes!("../web/lib/three/three.module.js")),
        "reader/lib/three/three.core.js" => (JS, include_bytes!("../web/lib/three/three.core.js")),
        "reader/lib/three/addons/loaders/GLTFLoader.js" => (JS, include_bytes!("../web/lib/three/addons/loaders/GLTFLoader.js")),
        "reader/lib/three/addons/loaders/OBJLoader.js" => (JS, include_bytes!("../web/lib/three/addons/loaders/OBJLoader.js")),
        "reader/lib/three/addons/loaders/STLLoader.js" => (JS, include_bytes!("../web/lib/three/addons/loaders/STLLoader.js")),
        "reader/lib/three/addons/controls/OrbitControls.js" => (JS, include_bytes!("../web/lib/three/addons/controls/OrbitControls.js")),
        "reader/lib/three/addons/utils/BufferGeometryUtils.js" => (JS, include_bytes!("../web/lib/three/addons/utils/BufferGeometryUtils.js")),
        "reader/lib/three/addons/utils/SkeletonUtils.js" => (JS, include_bytes!("../web/lib/three/addons/utils/SkeletonUtils.js")),
        "examples/manifest.json" => (JSON, include_bytes!("../examples/manifest.json")),
        "examples/quadratics.mdxb" => (BOOK, include_bytes!("../examples/quadratics.mdxb")),
        "examples/projectile-lab.mdxb" => (BOOK, include_bytes!("../examples/projectile-lab.mdxb")),
        "examples/type-specimen.mdxb" => (BOOK, include_bytes!("../examples/type-specimen.mdxb")),
        "examples/junior-physics.mdxb" => (BOOK, include_bytes!("../examples/junior-physics.mdxb")),
        "examples/junior-chemistry.mdxb" => (BOOK, include_bytes!("../examples/junior-chemistry.mdxb")),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(server: &ReaderServer, path: &str) -> String {
        let mut stream = TcpStream::connect(server.address).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn serves_only_reader_assets_at_random_loopback_path() {
        let server = ReaderServer::start().unwrap();
        let prefix = format!("/{}", server.token);
        let page = request(&server, &format!("{prefix}/reader/"));
        assert!(page.starts_with("HTTP/1.1 200 OK") && page.contains("MarkdownX"));
        let katex = request(&server, &format!("{prefix}/reader/js/katex.mjs"));
        assert!(katex.starts_with("HTTP/1.1 200 OK") && katex.contains("Content-Type: text/javascript") && katex.contains("renderToString"));
        let theme = request(&server, &format!("{prefix}/reader/js/theme.js"));
        assert!(theme.starts_with("HTTP/1.1 200 OK") && theme.contains("Content-Type: text/javascript") && theme.contains("readableAccent"));
        let manifest = request(&server, &format!("{prefix}/examples/manifest.json"));
        assert!(manifest.starts_with("HTTP/1.1 200 OK") && manifest.contains("books"));
        for path in ["/reader/", "/wrong/reader/", &format!("{prefix}/reader/../../Cargo.toml"), &format!("{prefix}/reader/%2e%2e/Cargo.toml")] {
            assert!(request(&server, path).starts_with("HTTP/1.1 404 Not Found"), "{path}");
        }
    }

    #[test]
    fn manifest_examples_are_available() {
        let manifest: serde_json::Value = serde_json::from_slice(include_bytes!("../examples/manifest.json")).unwrap();
        for book in manifest["books"].as_array().unwrap() {
            let path = format!("examples/{}", book["file"].as_str().unwrap());
            assert!(asset(&path).is_some(), "missing {path}");
        }
    }

    #[test]
    #[ignore = "manual browser smoke test"]
    fn browser_harness() {
        let server = ReaderServer::start().unwrap();
        println!("READER_URL={}", server.url());
        io::stdout().flush().unwrap();
        let signal = std::env::var_os("MARKDOWNX_READER_TEST_DONE").expect("set MARKDOWNX_READER_TEST_DONE to a temporary signal path");
        for _ in 0..600 {
            if std::path::Path::new(&signal).exists() { return; }
            thread::sleep(Duration::from_millis(100));
        }
        panic!("browser smoke test timed out");
    }
}
