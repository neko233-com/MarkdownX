//! Explicitly started extension process with bounded JSON-RPC transport.
//! The caller is responsible for trusting and enabling an installed extension.
use std::collections::BTreeSet;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::{Duration, Instant};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use crate::extensions::{PluginManifest, Registry};

pub const PROTOCOL_VERSION: u64 = 1;
/// A converted document is rendered in memory, so it is capped below the
/// transport limit instead of trusting the extension.
pub const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 32 * 1024;

#[derive(Debug)]
struct RemoteError { code: i64 }
impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "extension returned an error: {}", self.code) }
}
impl std::error::Error for RemoteError {}

pub struct ExtensionHost {
    child: Child,
    outbound: SyncSender<Value>,
    inbound: Receiver<Result<Value, String>>,
    /// Suffixes the manifest promises. `initialize` must advertise the same
    /// set, so a plugin cannot start serving a format it did not declare.
    suffixes: BTreeSet<String>,
    next_id: u64,
    initialized: bool,
    stopped: bool,
}

fn valid_path(path: &Path) -> Result<&str> {
    let text = path.to_str().context("extension document path is not valid UTF-8")?;
    ensure!(!text.is_empty() && text.len() <= MAX_PATH_BYTES && !text.contains('\0'), "invalid extension document path");
    Ok(text)
}

impl ExtensionHost {
    /// Start only an explicitly enabled manifest from its installed directory.
    /// A canonical path check prevents executable symlinks escaping that root.
    pub fn start(directory: &Path, manifest: &PluginManifest) -> Result<Self> {
        Registry::with_builtins().register(manifest.clone())?;
        let root = directory.canonicalize().context("extension directory unavailable")?;
        ensure!(root.is_dir(), "extension directory is not a directory");
        let executable = root.join(manifest.executable.replace('\\', "/")).canonicalize().context("extension executable unavailable")?;
        ensure!(executable.starts_with(&root) && executable.is_file(), "extension executable is outside its directory");
        let suffixes = manifest.normalized_suffixes()?;
        let mut command = Command::new(executable);
        command.args(&manifest.args).current_dir(&root).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
        let mut child = command.spawn().context("cannot start extension")?;
        let Some(stdin) = child.stdin.take() else { let _ = child.kill(); let _ = child.wait(); bail!("extension stdin unavailable") };
        let Some(stdout) = child.stdout.take() else { let _ = child.kill(); let _ = child.wait(); bail!("extension stdout unavailable") };
        let (outbound, messages) = mpsc::sync_channel::<Value>(1);
        let (responses, inbound) = mpsc::sync_channel::<Result<Value, String>>(32);
        let errors = responses.clone();
        std::thread::spawn(move || {
            let mut writer = BufWriter::new(stdin);
            while let Ok(message) = messages.recv() {
                if let Err(error) = crate::extension_rpc::write_message(&mut writer, &message) { let _ = errors.send(Err(error.to_string())); break; }
            }
        });
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let result = crate::extension_rpc::read_message(&mut reader).map_err(|error| error.to_string());
                let failed = result.is_err();
                if responses.send(result).is_err() || failed { break; }
            }
        });
        Ok(Self { child, outbound, inbound, suffixes, next_id: 1, initialized: false, stopped: false })
    }

    /// Start and complete the versioned handshake. A plugin that cannot
    /// initialize in time is stopped before the caller sees the error.
    pub fn start_ready(directory: &Path, manifest: &PluginManifest, timeout: Duration) -> Result<Self> {
        let mut host = Self::start(directory, manifest)?;
        if let Err(error) = host.initialize(timeout) { host.stop(); return Err(error); }
        Ok(host)
    }

    pub fn initialize(&mut self, timeout: Duration) -> Result<()> {
        ensure!(!self.initialized, "extension is already initialized");
        let result = self.request("initialize", json!({
            "protocolVersion": PROTOCOL_VERSION,
            "clientInfo": {"name": "MarkdownX", "version": env!("CARGO_PKG_VERSION")},
        }), timeout)?;
        ensure!(result.get("protocolVersion").and_then(Value::as_u64) == Some(PROTOCOL_VERSION), "unsupported extension protocol version");
        let advertised = result.get("suffixes").and_then(Value::as_array).context("extension did not advertise suffixes")?;
        ensure!(advertised.len() <= 64, "too many advertised extension suffixes");
        let mut suffixes = BTreeSet::new();
        for suffix in advertised {
            let suffix = suffix.as_str().context("invalid advertised extension suffix")?;
            ensure!(suffixes.insert(crate::extensions::normalize_suffix(suffix)?), "duplicate advertised extension suffix");
        }
        ensure!(suffixes == self.suffixes, "extension advertised suffixes different from its manifest");
        self.initialized = true;
        Ok(())
    }

    /// Ask for a Markdown rendering of `path`. The extension owns reading its
    /// own format; the returned text is displayed read-only.
    pub fn open_markdown(&mut self, path: &Path, timeout: Duration) -> Result<String> {
        ensure!(self.initialized, "extension is not initialized");
        let path = valid_path(path)?;
        let result = self.request("textDocument/open", json!({"path": path}), timeout)?;
        ensure!(result.get("format").and_then(Value::as_str) == Some("markdown"), "extension returned an unsupported document format");
        let text = result.get("text").and_then(Value::as_str).context("extension returned no document text")?;
        ensure!(text.len() <= MAX_DOCUMENT_BYTES, "extension document is too large");
        Ok(text.to_owned())
    }

    /// Hand edited Markdown back to the extension, which writes its own file.
    pub fn save_markdown(&mut self, path: &Path, text: &str, timeout: Duration) -> Result<()> {
        ensure!(self.initialized, "extension is not initialized");
        ensure!(text.len() <= MAX_DOCUMENT_BYTES, "extension document is too large");
        let path = valid_path(path)?;
        let result = self.request("textDocument/save", json!({"path": path, "format": "markdown", "text": text}), timeout)?;
        ensure!(result.get("ok").and_then(Value::as_bool) == Some(true), "extension did not confirm the save");
        Ok(())
    }

    /// Best-effort shutdown, then always stop the owned process.
    pub fn shutdown(&mut self, timeout: Duration) {
        if self.initialized && !self.stopped { let _ = self.request("shutdown", Value::Null, timeout); }
        self.stop();
    }

    /// Send one request, consuming notifications while waiting. A timeout,
    /// malformed response or broken transport stops this owned process so a
    /// late response cannot be mistaken for the next request.
    pub fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        ensure!(!self.stopped, "extension is stopped");
        ensure!(!method.is_empty() && method.len() <= 256 && !method.chars().any(char::is_control), "invalid extension method");
        ensure!(!timeout.is_zero() && timeout <= Duration::from_secs(120), "invalid extension timeout");
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or_else(|| anyhow::anyhow!("extension request id exhausted"))?;
        let result = self.exchange(id, method, params, timeout);
        if result.as_ref().err().is_some_and(|error| error.downcast_ref::<RemoteError>().is_none()) { self.stop(); }
        result
    }

    fn exchange(&mut self, id: u64, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.send(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))?;
        let deadline = Instant::now().checked_add(timeout).context("invalid extension deadline")?;
        let mut notifications = 0usize;
        loop {
            let remaining = deadline.checked_duration_since(Instant::now()).ok_or_else(|| anyhow::anyhow!("extension request timed out"))?;
            let message = self.inbound.recv_timeout(remaining).map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => anyhow::anyhow!("extension request timed out"),
                mpsc::RecvTimeoutError::Disconnected => anyhow::anyhow!("extension transport closed"),
            })?.map_err(anyhow::Error::msg)?;
            ensure!(message.get("jsonrpc").and_then(Value::as_str) == Some("2.0"), "invalid extension JSON-RPC version");
            if let Some(method) = message.get("method") {
                ensure!(method.as_str().is_some(), "invalid extension method");
                notifications += 1;
                ensure!(notifications <= 64, "too many extension messages");
                if let Some(server_id) = message.get("id") {
                    self.send(json!({"jsonrpc":"2.0", "id":server_id, "error":{"code":-32601,"message":"Method not supported"}}))?;
                }
                continue;
            }
            ensure!(message.get("id").and_then(Value::as_u64) == Some(id), "unexpected extension response id");
            let result = message.get("result");
            let error = message.get("error");
            ensure!(result.is_some() != error.is_some(), "invalid extension response");
            if let Some(error) = error { return Err(RemoteError { code: error.get("code").and_then(Value::as_i64).unwrap_or(-32603) }.into()); }
            return Ok(result.cloned().unwrap_or(Value::Null));
        }
    }

    fn send(&self, message: Value) -> Result<()> {
        match self.outbound.try_send(message) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => bail!("extension output queue is full"),
            Err(TrySendError::Disconnected(_)) => bail!("extension transport closed"),
        }
    }

    pub fn stop(&mut self) {
        if !self.stopped {
            self.stopped = true;
            if self.child.try_wait().ok().flatten().is_none() { let _ = self.child.kill(); }
            let _ = self.child.wait();
        }
    }
}

impl Drop for ExtensionHost { fn drop(&mut self) { self.stop(); } }
