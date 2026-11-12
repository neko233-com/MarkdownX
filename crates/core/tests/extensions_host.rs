//! End-to-end checks for the external extension lifecycle: discovery,
//! versioned handshake, document conversion, save-back and failure paths.
use fdl_core::extension_host::ExtensionHost;
use fdl_core::extensions::{InstalledPlugin, MANIFEST_FILE, PluginManifest, Provider, Registry, discover};
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("markdownx-ext-{name}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

fn fixture_manifest(args: &[&str]) -> PluginManifest {
    PluginManifest {
        schema_version: 1,
        id: "example.notes".into(),
        name: "Fixture notes".into(),
        suffixes: vec![".notes".into()],
        executable: format!("mdx-plugin-fixture{}", std::env::consts::EXE_SUFFIX),
        args: args.iter().map(|arg| (*arg).into()).collect(),
    }
}

/// Install the test binary inside its own plugin directory, as a real plugin
/// would sit after copying a folder into the plugins directory.
fn install_fixture(root: &Path, manifest: &PluginManifest) -> PathBuf {
    let directory = root.join(&manifest.id);
    std::fs::create_dir_all(&directory).unwrap();
    let source = env!("CARGO_BIN_EXE_mdx-plugin-fixture");
    std::fs::copy(source, directory.join(&manifest.executable)).unwrap();
    std::fs::write(directory.join(MANIFEST_FILE), serde_json::to_vec(manifest).unwrap()).unwrap();
    directory
}

#[test]
fn discovers_installs_and_round_trips_a_document() {
    let root = TempDir::new("roundtrip");
    let manifest = fixture_manifest(&["--suffixes", ".notes"]);
    let directory = install_fixture(&root.0, &manifest);

    let found = discover(&root.0);
    assert_eq!(found.len(), 1);
    let installed = found.into_iter().next().unwrap().unwrap();
    assert_eq!(installed.manifest, manifest);
    assert_eq!(installed.directory, directory);

    let (registry, rejected) = Registry::with_plugins([installed.clone()]);
    assert!(rejected.is_empty());
    assert!(matches!(registry.resolve(Path::new("表格.NOTES")), Some(Provider::External(item)) if item.id == "example.notes"));
    assert!(registry.resolve(Path::new("notes.md")).is_some());
    assert!(registry.manifest("example.notes").is_some());

    let document = root.0.join("角色.notes");
    std::fs::write(&document, "生命值 100\n攻击力 25\n").unwrap();
    let mut host = ExtensionHost::start_ready(&installed.directory, &installed.manifest, TIMEOUT).unwrap();
    let markdown = host.open_markdown(&document, TIMEOUT).unwrap();
    assert!(markdown.starts_with("# Opened by fixture"), "{markdown}");
    assert!(markdown.contains("生命值 100"));

    host.save_markdown(&document, "# 已更新\n", TIMEOUT).unwrap();
    assert_eq!(std::fs::read_to_string(&document).unwrap(), "# 已更新\n");
    host.shutdown(TIMEOUT);
    assert!(host.open_markdown(&document, TIMEOUT).is_err());
}

#[test]
fn rejects_a_handshake_that_does_not_match_the_manifest() {
    let root = TempDir::new("handshake");
    let manifest = fixture_manifest(&["--suffixes", ".other"]);
    let directory = install_fixture(&root.0, &manifest);
    let error = match ExtensionHost::start_ready(&directory, &manifest, TIMEOUT) { Ok(_) => panic!("handshake should fail"), Err(error) => error };
    assert!(error.to_string().contains("different"), "{error}");
}

#[test]
fn a_silent_extension_times_out_and_is_stopped() {
    let root = TempDir::new("timeout");
    let manifest = fixture_manifest(&["--suffixes", ".notes", "--silent"]);
    let directory = install_fixture(&root.0, &manifest);
    let started = std::time::Instant::now();
    assert!(ExtensionHost::start_ready(&directory, &manifest, Duration::from_millis(150)).is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn missing_or_escaping_executables_are_rejected() {
    let root = TempDir::new("unsafe");
    let mut manifest = fixture_manifest(&["--suffixes", ".notes"]);
    manifest.executable = "missing.exe".into();
    let directory = root.0.join(&manifest.id);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(MANIFEST_FILE), serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(ExtensionHost::start(&directory, &manifest).is_err());

    let mut escaping = manifest;
    escaping.executable = "../escape.exe".into();
    assert!(escaping.validate().is_err());
}

struct Fixture {
    root: TempDir,
    directory: PathBuf,
    manifest: PluginManifest,
}

impl Fixture {
    fn new(name: &str, args: &[&str]) -> Self {
        let root = TempDir::new(name);
        let manifest = fixture_manifest(args);
        let directory = install_fixture(&root.0, &manifest);
        Self { root, directory, manifest }
    }

    fn document(&self) -> PathBuf {
        let path = self.root.0.join("document.notes");
        std::fs::write(&path, "内容\n").unwrap();
        path
    }

    fn ready(&self) -> ExtensionHost { ExtensionHost::start_ready(&self.directory, &self.manifest, TIMEOUT).unwrap() }
}

#[test]
fn handshake_rejects_version_suffix_and_shape_violations() {
    for mode in ["wrong-version", "no-suffixes", "duplicate-suffixes", "extra-suffix"] {
        let fixture = Fixture::new(mode, &["--suffixes", ".notes", "--mode", mode]);
        assert!(ExtensionHost::start_ready(&fixture.directory, &fixture.manifest, TIMEOUT).is_err(), "{mode}");
    }
}

#[test]
fn handshake_normalizes_suffix_case() {
    let root = TempDir::new("uppercase");
    let mut manifest = fixture_manifest(&["--suffixes", ".notes", "--mode", "uppercase-suffix"]);
    manifest.suffixes = vec![".NOTES".into()];
    let directory = install_fixture(&root.0, &manifest);
    let mut host = ExtensionHost::start_ready(&directory, &manifest, TIMEOUT).unwrap();
    let document = root.0.join("note.notes");
    std::fs::write(&document, "大写后缀\n").unwrap();
    assert!(host.open_markdown(&document, TIMEOUT).unwrap().contains("大写后缀"));
    assert!(host.initialize(TIMEOUT).is_err(), "a second handshake must be rejected");
}

#[test]
fn remote_errors_keep_the_host_usable() {
    let fixture = Fixture::new("remote-error", &["--suffixes", ".notes", "--mode", "open-error"]);
    let mut host = fixture.ready();
    assert!(host.open_markdown(&fixture.document(), TIMEOUT).is_err());
    assert!(host.open_markdown(&fixture.document(), TIMEOUT).is_err(), "the host must survive an extension error response");
}

#[test]
fn malformed_documents_are_rejected_without_a_false_success() {
    for mode in ["wrong-format", "no-text", "large-text"] {
        let fixture = Fixture::new(mode, &["--suffixes", ".notes", "--mode", mode]);
        let mut host = fixture.ready();
        assert!(host.open_markdown(&fixture.document(), TIMEOUT).is_err(), "{mode}");
    }
}

#[test]
fn open_timeouts_and_early_exits_stop_the_process() {
    let slow = Fixture::new("slow-open", &["--suffixes", ".notes", "--sleep-ms", "1500"]);
    let mut host = slow.ready();
    let started = std::time::Instant::now();
    assert!(host.open_markdown(&slow.document(), Duration::from_millis(150)).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(host.open_markdown(&slow.document(), TIMEOUT).is_err(), "a timed-out host stays stopped");

    let exiting = Fixture::new("exit-open", &["--suffixes", ".notes", "--mode", "exit-on-open"]);
    let mut host = exiting.ready();
    assert!(host.open_markdown(&exiting.document(), TIMEOUT).is_err());
    assert!(host.open_markdown(&exiting.document(), TIMEOUT).is_err());
}

#[test]
fn save_confirmation_and_shutdown_are_enforced() {
    let fixture = Fixture::new("save-false", &["--suffixes", ".notes", "--mode", "save-false"]);
    let mut host = fixture.ready();
    assert!(host.save_markdown(&fixture.document(), "text", TIMEOUT).is_err());
    assert!(host.save_markdown(&fixture.document(), "text", TIMEOUT).is_err());
    host.shutdown(TIMEOUT);
    host.shutdown(TIMEOUT);
    assert!(host.open_markdown(&fixture.document(), TIMEOUT).is_err());
    assert!(host.save_markdown(&fixture.document(), "text", TIMEOUT).is_err());
}

#[test]
fn notifications_before_a_result_are_consumed() {
    let fixture = Fixture::new("notify", &["--suffixes", ".notes", "--mode", "notify-open"]);
    let mut host = fixture.ready();
    let markdown = host.open_markdown(&fixture.document(), TIMEOUT).unwrap();
    assert!(markdown.starts_with("# Opened by fixture"), "{markdown}");
}

#[test]
fn a_conflicting_manifest_is_rejected_as_a_whole() {
    let manifest = PluginManifest {
        schema_version: 1,
        id: "example.conflict".into(),
        name: "Conflict".into(),
        suffixes: vec![".notes".into(), ".md".into()],
        executable: "server".into(),
        args: vec![],
    };
    let mut registry = Registry::with_builtins();
    assert!(registry.register(manifest).is_err());
    assert!(registry.resolve(Path::new("file.notes")).is_none());
    assert_eq!(registry.resolve(Path::new("file.md")), Some(&Provider::Builtin(fdl_core::extensions::BuiltinEditor::Markdown)));
    assert!(InstalledPlugin { directory: PathBuf::from("x"), manifest: fixture_manifest(&[]) }.manifest.validate().is_ok());
}
