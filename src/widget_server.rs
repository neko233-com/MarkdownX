//! A private, short-lived loopback origin for an interactive document block.
//! Only explicitly referenced media is served; the iframe cannot make script
//! requests, navigate, or access the application's origin.
use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use base64::{engine::general_purpose::STANDARD, Engine};

const HEADER_LIMIT: usize = 8192;
const MAX_CLIENTS: usize = 8;

struct Asset { path: PathBuf, mime: &'static str, content: Option<Vec<u8>> }

/// The caller owns the origin and stops it when the widget is disposed.
pub struct WidgetServer { address: SocketAddr, token: String, stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>> }

impl WidgetServer {
    pub fn start(source: &str, base: &Path, root: &Path) -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let address = listener.local_addr()?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let origin = format!("http://{address}");
        let mut assets = HashMap::new();
        let mut inline_bytes = 0usize;
        let rewritten = fdl_core::html_assets::rewrite_references(source, |reference| {
            let script = reference.script_parts();
            let view = fdl_core::html_assets::svg_fragment(&reference.value);
            let (resource, mut fragment) = reference.media_time().or(script).or(view).unwrap_or((&reference.value, ""));
            let literal = if view.is_some() {
                match local_path(&reference.value, base, root) {
                    Ok(Some(path)) => Some(path),
                    Ok(None) => None,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error),
                }
            } else { None };
            let path = if let Some(path) = literal { fragment = ""; path }
                else { let Some(path) = local_path(resource, base, root)? else { return Ok::<_, io::Error>(None) }; path };
            if reference.javascript {
                let content = fdl_core::js_assets::bundle_file_for_document(&path, base, root, 16 * 1024 * 1024, reference.module).map_err(io::Error::other)?;
                inline_bytes = inline_bytes.saturating_add(content.len());
                if inline_bytes > 64 * 1024 * 1024 { return Err(io::Error::other("script dependencies are too large")); }
                let identity = if reference.module && script.is_some() { fdl_core::html_assets::script_identity(fragment) } else { String::new() };
                return Ok(Some(format!("data:text/javascript;base64,{}{identity}", STANDARD.encode(content))));
            }
            let id = assets.len();
            let mime = if reference.tag == "link" { "text/css" } else if reference.module { "text/javascript" } else { mime(&path) };
            let content = if reference.tag == "link" {
                Some(fdl_core::css_assets::bundle_file(&path, root, 16 * 1024 * 1024).map_err(io::Error::other)?)
            } else { None };
            assets.insert(format!("/{token}/asset/{id}"), Asset { path, mime, content });
            Ok(Some(format!("{origin}/{token}/asset/{id}{}", if script.is_some() { "" } else { fragment })))
        })?;
        let rewritten = fdl_core::html_assets::rewrite_inline_styles(&rewritten, |css| {
            let bundled = fdl_core::css_assets::bundle_inline(css, base, root, 16 * 1024 * 1024).map_err(io::Error::other)?;
            inline_bytes = inline_bytes.saturating_add(bundled.len().saturating_sub(css.len()));
            if inline_bytes > 64 * 1024 * 1024 { return Err(io::Error::other("inline stylesheet dependencies are too large")); }
            Ok::<_, io::Error>((bundled != css).then_some(bundled))
        })?;
        let rewritten = fdl_core::html_assets::rewrite_inline_scripts(&rewritten, |script, module| {
            if !module && !fdl_core::js_assets::may_reference_local_assets(script) { return Ok::<_, io::Error>(None); }
            let bundled = fdl_core::js_assets::bundle_inline(script, base, root, 16 * 1024 * 1024).map_err(io::Error::other)?;
            inline_bytes = inline_bytes.saturating_add(bundled.len().saturating_sub(script.len()));
            if inline_bytes > 64 * 1024 * 1024 { return Err(io::Error::other("inline module dependencies are too large")); }
            Ok::<_, io::Error>((bundled != script).then_some(bundled))
        })?;
        let html = page(&rewritten, &origin);
        let route = format!("/{token}/index.html");
        let stop = Arc::new(AtomicBool::new(false));
        let ending = stop.clone();
        let thread = thread::Builder::new().name("markdownx-widget".into()).spawn(move || {
            let assets = Arc::new(assets);
            let html = Arc::new(html.into_bytes());
            let active = Arc::new(AtomicUsize::new(0));
            while !ending.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if ending.load(Ordering::Acquire) { break; }
                        if active.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
                            active.fetch_sub(1, Ordering::AcqRel);
                            continue;
                        }
                        let assets = assets.clone();
                        let html = html.clone();
                        let route = route.clone();
                        let active = active.clone();
                        let count = active.clone();
                        if thread::Builder::new().name("markdownx-widget-request".into()).spawn(move || {
                            let _ = serve(stream, &route, &html, &assets);
                            active.fetch_sub(1, Ordering::AcqRel);
                        }).is_err() { count.fetch_sub(1, Ordering::AcqRel); }
                    }
                    Err(_) => break,
                }
            }
        })?;
        Ok(Self { address, token, stop, thread: Some(thread) })
    }

    pub fn url(&self) -> String { format!("http://{}/{}/index.html", self.address, self.token) }
}

impl Drop for WidgetServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(100));
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}

fn local_path(src: &str, base: &Path, root: &Path) -> io::Result<Option<PathBuf>> {
    let src = if src.contains('#') || Path::new(src).is_absolute() && Path::new(src).is_file() { src } else { src.split_once('?').map_or(src, |(path, _)| path) };
    let path = Path::new(src);
    let path = if path.is_absolute() { path.to_path_buf() }
        else if let Ok(url) = url::Url::parse(src) {
            if url.scheme() != "file" { return Ok(None); }
            url.to_file_path().map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, crate::settings::tr("本地互动资源路径无效", "Invalid local interactive resource path")))?
        } else if src.is_empty() || src.starts_with('#') || src.contains(':') { return Ok(None); }
        else {
            let decoded = crate::transfer::percent_decode(src);
            let local = base.join(&decoded);
            if !src.starts_with("./") && decoded.starts_with("assets/") && !local.is_file() { root.join(decoded) } else { local }
        };
    let root = root.canonicalize()?;
    let path = path.canonicalize().map_err(|_| io::Error::new(io::ErrorKind::NotFound, crate::settings::tr("本地互动资源不存在或无法读取", "Local interactive resource is missing or inaccessible")))?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, crate::settings::tr("本地互动资源不在工作区内", "Local interactive resource is outside the workspace")));
    }
    Ok(Some(path))
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "gif" => "image/gif", "svg" => "image/svg+xml",
        "webp" => "image/webp", "avif" => "image/avif", "bmp" => "image/bmp", "ico" => "image/x-icon",
        "mp4" | "m4v" => "video/mp4", "webm" => "video/webm", "ogv" => "video/ogg", "mov" => "video/quicktime",
        "mp3" => "audio/mpeg", "m4a" => "audio/mp4", "wav" => "audio/wav", "ogg" | "opus" => "audio/ogg",
        "vtt" => "text/vtt", "js" | "mjs" => "text/javascript", "css" => "text/css", _ => "application/octet-stream",
    }
}

fn page(source: &str, origin: &str) -> String {
    let inner = format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' data: blob: {origin}; style-src 'unsafe-inline' data: {origin}; img-src data: blob: {origin}; media-src data: blob: {origin}; font-src data:; connect-src data:; frame-src 'none'; form-action 'none'; base-uri 'none'\"><style>html,body{{margin:0;height:100%;font:14px/1.5 system-ui,sans-serif;color:#222;background:#fff}}body{{display:grid;place-items:center;padding:16px;box-sizing:border-box}}video,img{{max-width:100%;max-height:100%}}audio{{max-width:100%}}</style></head><body>{source}</body></html>");
    let json = serde_json::to_string(&inner).expect("string serialization").replace('<', "\\u003c").replace('>', "\\u003e").replace('&', "\\u0026").replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029");
    format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' data: blob: {origin}; style-src 'unsafe-inline' data: {origin}; img-src data: blob: {origin}; media-src data: blob: {origin}; font-src data:; connect-src data:; frame-src 'self'; form-action 'none'; base-uri 'none'\"><style>html,body,iframe{{margin:0;border:0;width:100%;height:100%;overflow:hidden}}</style></head><body><iframe sandbox=\"allow-scripts\" title=\"Interactive content\"></iframe><script>document.querySelector('iframe').srcdoc={json};</script></body></html>")
}

fn serve(mut stream: TcpStream, route: &str, html: &[u8], assets: &HashMap<String, Asset>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut header = Vec::new();
    let mut buf = [0u8; 1024];
    while !header.windows(4).any(|part| part == b"\r\n\r\n") {
        let n = stream.read(&mut buf)?;
        if n == 0 || header.len() + n > HEADER_LIMIT { return Ok(()); }
        header.extend_from_slice(&buf[..n]);
    }
    let Ok(header) = std::str::from_utf8(&header) else { return Ok(()) };
    let mut lines = header.split("\r\n");
    let mut request = lines.next().unwrap_or("").split_whitespace();
    let method = request.next().unwrap_or("");
    let path = request.next().unwrap_or("");
    if !matches!(method, "GET" | "HEAD") || request.next().is_none() || request.next().is_some() { return reply(&mut stream, "405 Method Not Allowed", "text/plain", b"", method == "HEAD"); }
    let range = lines.filter_map(|line| line.split_once(':')).find(|(key, _)| key.eq_ignore_ascii_case("range")).map(|(_, value)| value.trim());
    if path == route { return reply(&mut stream, "200 OK", "text/html; charset=utf-8", html, method == "HEAD"); }
    let Some(asset) = assets.get(path) else { return reply(&mut stream, "404 Not Found", "text/plain", b"", method == "HEAD") };
    trait ReadSeek: Read + Seek {}
    impl<T: Read + Seek> ReadSeek for T {}
    let (mut file, len): (Box<dyn ReadSeek + '_>, u64) = if let Some(content) = &asset.content {
        (Box::new(std::io::Cursor::new(content)), content.len() as u64)
    } else {
        let Ok(file) = std::fs::File::open(&asset.path) else { return reply(&mut stream, "404 Not Found", "text/plain", b"", method == "HEAD") };
        let meta = file.metadata()?;
        if !meta.is_file() { return reply(&mut stream, "404 Not Found", "text/plain", b"", method == "HEAD"); }
        (Box::new(file), meta.len())
    };
    let (start, end) = match range { Some(value) => match byte_range(value, len) { Some(pair) => pair, None => {
        write!(stream, "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?; return Ok(());
    } }, None if len > 0 => (0, len - 1), None => (0, 0) };
    let count = if len == 0 { 0 } else { end - start + 1 };
    let status = if range.is_some() { "206 Partial Content" } else { "200 OK" };
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {}\r\nContent-Length: {count}\r\nAccept-Ranges: bytes\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n", asset.mime)?;
    if range.is_some() { write!(stream, "Content-Range: bytes {start}-{end}/{len}\r\n")?; }
    write!(stream, "\r\n")?;
    if method != "HEAD" && count > 0 { file.seek(SeekFrom::Start(start))?; io::copy(&mut file.take(count), &mut stream)?; }
    Ok(())
}

fn reply(stream: &mut TcpStream, status: &str, mime: &str, body: &[u8], head: bool) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n", body.len())?;
    if !head { stream.write_all(body)?; }
    Ok(())
}

fn byte_range(value: &str, len: u64) -> Option<(u64, u64)> {
    if len == 0 { return None; }
    let (first, last) = value.strip_prefix("bytes=")?.split_once('-')?;
    if first.is_empty() { let suffix = last.parse::<u64>().ok()?; return (suffix > 0).then_some((len.saturating_sub(suffix), len - 1)); }
    let start = first.parse::<u64>().ok()?;
    if start >= len { return None; }
    let end = if last.is_empty() { len - 1 } else { last.parse::<u64>().ok()?.min(len - 1) };
    (end >= start).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(server: &WidgetServer, path: &str, headers: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(server.address).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {}\r\n{headers}\r\n", server.address).unwrap();
        let mut out = Vec::new();
        stream.read_to_end(&mut out).unwrap();
        out
    }

    #[test]
    fn serves_only_referenced_assets_and_supports_video_seeking() {
        let path = std::env::temp_dir().join(format!("markdownx-widget-{}.mp4", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"0123456789").unwrap();
        let source = format!("<video src='{}' controls></video><script>const x = '</script>';</script>", path.display());
        let server = WidgetServer::start(&source, &std::env::temp_dir(), &std::env::temp_dir()).unwrap();
        let root = format!("/{}/index.html", server.token);
        let page = String::from_utf8(request(&server, &root, "")).unwrap();
        assert!(page.starts_with("HTTP/1.1 200 OK"));
        assert!(page.contains("sandbox=\"allow-scripts\""));
        assert_eq!(page.matches("connect-src data:").count(), 2);
        assert!(!page.contains(&path.to_string_lossy().to_string()));
        let asset = format!("/{}/asset/0", server.token);
        let response = request(&server, &asset, "Range: bytes=3-5\r\n");
        assert!(response.starts_with(b"HTTP/1.1 206 Partial Content\r\n"));
        assert!(response.windows(b"Content-Range: bytes 3-5/10".len()).any(|s| s == b"Content-Range: bytes 3-5/10"));
        assert!(response.ends_with(b"345"));
        assert!(request(&server, "/not-the-token/asset/0", "").starts_with(b"HTTP/1.1 404"));
        assert!(request(&server, &asset, "Range: bytes=50-\r\n").starts_with(b"HTTP/1.1 416"));
        assert_eq!(std::fs::read(&path).unwrap(), b"0123456789");
        drop(server);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn module_dependencies_are_bundled_for_the_sandbox() {
        let root = std::env::temp_dir().join(format!("markdownx-widget-module-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("helper.mjs"), "export const n=42;").unwrap();
        std::fs::write(root.join("frame.svg"), "<svg/>").unwrap();
        std::fs::write(root.join("game.mjs"), "import {n} from './helper.mjs'; const art=new URL('frame.svg',import.meta.url); console.log(n);").unwrap();
        let source = "<canvas></canvas><script type='module' src='./game.mjs'></script><script type='module'>import './helper.mjs';</script>";
        let server = WidgetServer::start(source, &root, &root).unwrap();
        let html = String::from_utf8(request(&server, &format!("/{}/index.html", server.token), "")).unwrap();
        assert!(html.contains("data:text/javascript;base64,"));
        let urls = regex::Regex::new(r"data:text/javascript;base64,([A-Za-z0-9+/=]+)").unwrap();
        let decoded: Vec<_> = urls.captures_iter(&html).filter_map(|m| STANDARD.decode(&m[1]).ok()).collect();
        assert!(decoded.iter().any(|script| script.windows(b"data:text/javascript;base64,".len()).any(|v| v == b"data:text/javascript;base64,")
            && script.windows(b"data:image/svg+xml;base64,PHN2Zy8+".len()).any(|v| v == b"data:image/svg+xml;base64,PHN2Zy8+")));
        drop(server);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn media_and_scripts_stay_inside_the_workspace_and_use_the_document_directory() {
        let root = std::env::temp_dir().join(format!("markdownx-widget-boundary-{}", uuid::Uuid::new_v4()));
        let base = root.join("chapter");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(root.join("clip.mp4"), b"root video").unwrap();
        std::fs::write(base.join("clip.mp4"), b"chapter video").unwrap();
        std::fs::write(root.join("shared.js"), b"shared script").unwrap();
        let outside = root.with_extension("js");
        std::fs::write(&outside, b"outside script").unwrap();
        let source = "<video src='./clip.mp4'></video><script src='../shared.js'></script>";
        let server = WidgetServer::start(source, &base, &root).unwrap();
        assert!(request(&server, &format!("/{}/asset/0", server.token), "").ends_with(b"chapter video"));
        let page = request(&server, &format!("/{}/index.html", server.token), "");
        assert!(String::from_utf8_lossy(&page).contains(&STANDARD.encode(b"shared script")));
        drop(server);
        let bare = WidgetServer::start("<video src='clip.mp4'></video>", &base, &root).unwrap();
        assert!(request(&bare, &format!("/{}/asset/0", bare.token), "").ends_with(b"chapter video"));
        drop(bare);
        let queried = WidgetServer::start("<video src='./clip.mp4?v=1'><source src='clip.mp4?cache=2'></video>", &base, &root).unwrap();
        assert!(request(&queried, &format!("/{}/asset/0", queried.token), "").ends_with(b"chapter video"));
        assert!(request(&queried, &format!("/{}/asset/1", queried.token), "").ends_with(b"chapter video"));
        drop(queried);
        let timed = WidgetServer::start("<video src='./clip.mp4?v=1#t=2,5'></video>", &base, &root).unwrap();
        let page = request(&timed, &format!("/{}/index.html", timed.token), "");
        assert!(String::from_utf8_lossy(&page).contains("#t=2,5"));
        assert!(request(&timed, &format!("/{}/asset/0", timed.token), "").ends_with(b"chapter video"));
        drop(timed);
        let script = WidgetServer::start("<canvas></canvas><script src='../shared.js?v=1'></script>", &base, &root).unwrap();
        assert!(String::from_utf8_lossy(&request(&script, &format!("/{}/index.html", script.token), "")).contains(&STANDARD.encode(b"shared script")));
        drop(script);
        std::fs::write(base.join("module.mjs"), "window.runs=(window.runs||0)+1;").unwrap();
        let modules = WidgetServer::start("<canvas></canvas><script type='module' src='./module.mjs?v=1'></script><script type='module' src='./module.mjs?v=1'></script><script type='module' src='./module.mjs?v=2'></script>", &base, &root).unwrap();
        let page = String::from_utf8_lossy(&request(&modules, &format!("/{}/index.html", modules.token), "")).to_string();
        assert_eq!(page.matches("#%3F%76%3D%31").count(), 2);
        assert_eq!(page.matches("#%3F%76%3D%32").count(), 1);
        drop(modules);
        std::fs::write(base.join("frame.svg"), b"chapter frame").unwrap();
        let responsive = WidgetServer::start("<canvas></canvas><picture><source srcset='./frame.svg 1x, frame.svg 2x'></picture>", &base, &root).unwrap();
        assert!(request(&responsive, &format!("/{}/asset/0", responsive.token), "").ends_with(b"chapter frame"));
        assert!(request(&responsive, &format!("/{}/asset/1", responsive.token), "").ends_with(b"chapter frame"));
        drop(responsive);
        std::fs::write(base.join("frame#take.svg"), b"chapter SVG view").unwrap();
        let views = WidgetServer::start("<canvas></canvas><img src='./frame%23take.svg?v=1#right'>", &base, &root).unwrap();
        let view_page = String::from_utf8_lossy(&request(&views, &format!("/{}/index.html", views.token), "")).to_string();
        assert!(view_page.contains("#right"));
        assert!(request(&views, &format!("/{}/asset/0", views.token), "").ends_with(b"chapter SVG view"));
        drop(views);
        std::fs::create_dir(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/shared.js"), b"book asset").unwrap();
        let fallback = WidgetServer::start("<canvas></canvas><script src='assets/shared.js'></script>", &base, &root).unwrap();
        assert!(String::from_utf8_lossy(&request(&fallback, &format!("/{}/index.html", fallback.token), "")).contains(&STANDARD.encode(b"book asset")));
        drop(fallback);
        assert_eq!(WidgetServer::start("<canvas></canvas><script src='./assets/shared.js'></script>", &base, &root).err().unwrap().kind(), io::ErrorKind::NotFound);
        let outside_source = format!("<canvas></canvas><script src='{}'></script>", outside.display());
        assert_eq!(WidgetServer::start(&outside_source, &base, &root).err().unwrap().kind(), io::ErrorKind::PermissionDenied);
        let outside_query = format!("<script src='../../{}?v=1'></script>", outside.file_name().unwrap().to_string_lossy());
        assert_eq!(WidgetServer::start(&outside_query, &base, &root).err().unwrap().kind(), io::ErrorKind::PermissionDenied);
        let outside_url = url::Url::from_file_path(&outside).unwrap();
        assert_eq!(WidgetServer::start(&format!("<video src='{outside_url}'></video>"), &base, &root).err().unwrap().kind(), io::ErrorKind::PermissionDenied);
        let traversal = format!("<video src='../../{}'></video>", outside.file_name().unwrap().to_string_lossy());
        assert_eq!(WidgetServer::start(&traversal, &base, &root).err().unwrap().kind(), io::ErrorKind::PermissionDenied);
        let traversal = format!("<video src='../../{}?v=1#t=2,5'></video>", outside.file_name().unwrap().to_string_lossy());
        assert_eq!(WidgetServer::start(&traversal, &base, &root).err().unwrap().kind(), io::ErrorKind::PermissionDenied);
        let traversal = format!("<video src='../../{}?v=1'></video>", outside.file_name().unwrap().to_string_lossy());
        assert_eq!(WidgetServer::start(&traversal, &base, &root).err().unwrap().kind(), io::ErrorKind::PermissionDenied);
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&outside, base.join("link.js")).is_ok() {
            assert_eq!(WidgetServer::start("<canvas></canvas><script src='./link.js'></script>", &base, &root).err().unwrap().kind(), io::ErrorKind::PermissionDenied);
        }
        assert_eq!(WidgetServer::start("<video src='./missing.mp4'></video>", &base, &root).err().unwrap().kind(), io::ErrorKind::NotFound);
        assert!(WidgetServer::start("<video src='https://example.invalid/clip.mp4'></video>", &base, &root).is_ok());
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside script");
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn serves_bundled_stylesheet_dependencies_inside_workspace() {
        let root = std::env::temp_dir().join(format!("markdownx-widget-css-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("styles")).unwrap();
        std::fs::create_dir_all(root.join("images")).unwrap();
        let css = root.join("styles/game");
        let original = "canvas { background-image: url('../images/frame.svg') }";
        std::fs::write(&css, original).unwrap();
        std::fs::write(root.join("images/frame.svg"), b"<svg/>").unwrap();
        let html = format!("<canvas></canvas><link rel='stylesheet' href='{}'>", css.display());
        let server = WidgetServer::start(&html, &root, &root).unwrap();
        let response = request(&server, &format!("/{}/asset/0", server.token), "");
        assert!(response.windows(b"data:image/svg+xml;base64,PHN2Zy8+".len()).any(|s| s == b"data:image/svg+xml;base64,PHN2Zy8+"));
        assert_eq!(std::fs::read_to_string(&css).unwrap(), original);
        drop(server);
        std::fs::write(&css, "canvas { background: url('../../private.svg') }").unwrap();
        assert!(WidgetServer::start(&html, &root, &root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inline_styles_use_document_base_and_preserve_source() {
        let root = std::env::temp_dir().join(format!("markdownx-inline-widget-{}", uuid::Uuid::new_v4()));
        let base = root.join("chapter");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(root.join("frame.svg"), b"wrong").unwrap();
        std::fs::write(base.join("frame.svg"), b"<svg/>").unwrap();
        let source = "<canvas style=\"background:url('./frame.svg')\"></canvas><style>canvas { border-image:url(frame.svg) }</style><!-- url(missing.png) --><script>let s='url(missing.png)'</script>";
        let server = WidgetServer::start(source, &base, &root).unwrap();
        let page = String::from_utf8(request(&server, &format!("/{}/index.html", server.token), "")).unwrap();
        assert_eq!(page.matches("data:image/svg+xml;base64,PHN2Zy8+").count(), 2);
        assert!(page.contains("url(missing.png)"));
        assert!(source.contains("url('./frame.svg')"));
        drop(server);
        assert!(WidgetServer::start("<style>a{background:url(missing.png)}</style>", &base, &root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn handles_ranges_and_script_text_without_escaping_the_wrapper() {
        assert_eq!(byte_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(byte_range("bytes=5-99", 10), Some((5, 9)));
        assert_eq!(byte_range("bytes=9-3", 10), None);
        assert_eq!(byte_range("bytes=0-1,3-4", 10), None);
        let page = page("</script><script>bad()</script>", "http://127.0.0.1:1234");
        assert_eq!(page.matches("</script>").count(), 1);
        assert!(page.contains("\\u003c/script\\u003e"));
    }
}
