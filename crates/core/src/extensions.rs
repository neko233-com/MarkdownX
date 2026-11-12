//! File-extension provider registry. External providers are described by a
//! versioned manifest; registration never starts a process or opens a document.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

pub const MANIFEST_FILE: &str = "plugin.json";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinEditor { Markdown, DocxXml, Book, Spreadsheet, Image, Audio, Video, Model }

/// Suffixes served without launching another process. Registration order is
/// also the order the settings page shows.
pub const BUILTINS: &[(&str, BuiltinEditor, &[&str])] = &[
    ("builtin.markdown", BuiltinEditor::Markdown, &[".md", ".markdown", ".mark", ".txt"]),
    ("builtin.docxxml", BuiltinEditor::DocxXml, &[".docxxml"]),
    ("builtin.book", BuiltinEditor::Book, &[".mdxb"]),
    ("builtin.spreadsheet", BuiltinEditor::Spreadsheet, &[".xlsx", ".xlsm", ".xlam"]),
    ("builtin.image", BuiltinEditor::Image, &[".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg", ".ico", ".avif", ".tif", ".tiff"]),
    ("builtin.audio", BuiltinEditor::Audio, &[".mp3", ".wav", ".flac", ".ogg", ".m4a", ".aac", ".opus", ".wma"]),
    ("builtin.video", BuiltinEditor::Video, &[".mp4", ".webm", ".mov", ".m4v", ".mkv", ".avi", ".wmv"]),
    ("builtin.model", BuiltinEditor::Model, &[".glb", ".gltf", ".obj", ".stl"]),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    /// Suffixes include their leading dot; compound suffixes are supported.
    pub suffixes: Vec<String>,
    /// An executable relative to the installed plugin directory. Registration
    /// does not grant permission to execute it.
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provider { Builtin(BuiltinEditor), External(PluginManifest) }

/// A validated manifest together with the directory that contains it. The
/// executable is only resolved when the host starts, after the user enabled it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledPlugin { pub directory: PathBuf, pub manifest: PluginManifest }

impl PluginManifest {
    /// Checks that do not depend on what is already registered.
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "unsupported plugin manifest version");
        ensure!(valid_id(&self.id) && !self.id.starts_with("builtin."), "invalid or reserved plugin id");
        ensure!(!self.name.trim().is_empty() && self.name.len() <= 256, "invalid plugin name");
        ensure!(!self.suffixes.is_empty() && self.suffixes.len() <= 64, "invalid plugin suffix count");
        ensure!(self.args.len() <= 64 && self.args.iter().all(|arg| arg.len() <= 4096 && !arg.contains('\0')), "invalid plugin arguments");
        validate_executable(&self.executable)?;
        let mut unique = BTreeSet::new();
        for suffix in &self.suffixes { ensure!(unique.insert(normalize_suffix(suffix)?), "duplicate extension suffix in manifest"); }
        Ok(())
    }

    pub fn normalized_suffixes(&self) -> Result<BTreeSet<String>> {
        self.suffixes.iter().map(|suffix| normalize_suffix(suffix)).collect()
    }
}

/// Read every immediate child directory that contains `plugin.json`. One bad
/// plugin is returned as its own error so it cannot hide the others.
pub fn discover(root: &Path) -> Vec<Result<InstalledPlugin>> {
    let Ok(entries) = std::fs::read_dir(root) else { return vec![] };
    let mut found = vec![];
    for entry in entries.flatten() {
        let directory = entry.path();
        let manifest = directory.join(MANIFEST_FILE);
        if !directory.is_dir() || !manifest.is_file() { continue; }
        found.push(load_manifest(&directory, &manifest));
    }
    found.sort_by_key(|item| item.as_ref().map(|plugin| plugin.manifest.id.clone()).unwrap_or_default());
    found
}

fn load_manifest(directory: &Path, path: &Path) -> Result<InstalledPlugin> {
    let meta = std::fs::metadata(path).context("cannot inspect plugin manifest")?;
    ensure!(meta.is_file() && meta.len() <= MAX_MANIFEST_BYTES, "invalid plugin manifest size");
    let bytes = std::fs::read(path).context("cannot read plugin manifest")?;
    let manifest: PluginManifest = serde_json::from_slice(&bytes).context("invalid plugin manifest JSON")?;
    manifest.validate()?;
    Ok(InstalledPlugin { directory: directory.to_path_buf(), manifest })
}

#[derive(Clone, Debug, Default)]
pub struct Registry { providers: BTreeMap<String, Provider>, suffixes: BTreeMap<String, String> }

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_' | b'.'))
        && id.as_bytes()[0].is_ascii_alphanumeric() && !id.ends_with('.') && !id.contains("..")
}

pub(crate) fn normalize_suffix(suffix: &str) -> Result<String> {
    ensure!(suffix.len() >= 2 && suffix.len() <= 128 && suffix.starts_with('.'), "invalid extension suffix");
    let normalized = suffix.to_ascii_lowercase();
    ensure!(normalized[1..].bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
        && !normalized.ends_with('.') && !normalized.contains(".."), "invalid extension suffix");
    Ok(normalized)
}

fn validate_executable(path: &str) -> Result<()> {
    ensure!(!path.is_empty() && path.len() <= 1024 && !path.contains(['\0', ':']), "invalid plugin executable");
    let normalized = path.replace('\\', "/");
    ensure!(!normalized.starts_with('/') && normalized.split('/').all(|part| !part.is_empty() && part != "." && part != ".."), "plugin executable must be inside its directory");
    Ok(())
}

impl Registry {
    pub fn with_builtins() -> Self {
        let mut registry = Self::default();
        for (id, editor, suffixes) in BUILTINS {
            registry.providers.insert((*id).into(), Provider::Builtin(*editor));
            for suffix in *suffixes { registry.suffixes.insert((*suffix).into(), (*id).into()); }
        }
        registry
    }

    /// Add every enabled plugin in discovery order. A plugin that conflicts
    /// with a builtin, another plugin, or an earlier duplicate is skipped as a
    /// whole, so registration never leaves a partial suffix map behind.
    pub fn with_plugins<I>(plugins: I) -> (Self, Vec<(PathBuf, String)>)
    where I: IntoIterator<Item = InstalledPlugin> {
        let mut registry = Self::with_builtins();
        let mut rejected = vec![];
        for plugin in plugins {
            let directory = plugin.directory;
            if let Err(error) = registry.register(plugin.manifest) { rejected.push((directory, error.to_string())); }
        }
        (registry, rejected)
    }

    /// Validate the entire manifest before changing the registry. Existing
    /// providers cannot be silently replaced by another plugin.
    pub fn register(&mut self, manifest: PluginManifest) -> Result<()> {
        manifest.validate()?;
        ensure!(!self.providers.contains_key(&manifest.id), "duplicate plugin id");
        let suffixes = manifest.normalized_suffixes()?;
        for suffix in &suffixes { ensure!(!self.suffixes.contains_key(suffix), "extension suffix is already registered"); }
        for suffix in suffixes { self.suffixes.insert(suffix, manifest.id.clone()); }
        self.providers.insert(manifest.id.clone(), Provider::External(manifest));
        Ok(())
    }

    pub fn manifest(&self, id: &str) -> Option<&PluginManifest> {
        match self.providers.get(id) { Some(Provider::External(manifest)) => Some(manifest), _ => None }
    }

    /// Suffixes served by external providers, for the open-file dialog.
    pub fn external_suffixes(&self) -> Vec<String> {
        self.suffixes.iter().filter(|(_, id)| matches!(self.providers.get(*id), Some(Provider::External(_))))
            .map(|(suffix, _)| suffix.clone()).collect()
    }

    pub fn resolve(&self, path: &Path) -> Option<&Provider> {
        let filename = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        self.suffixes.iter().filter(|(suffix, _)| filename.len() > suffix.len() && filename.ends_with(suffix.as_str()))
            .max_by_key(|(suffix, _)| suffix.len()).and_then(|(_, id)| self.providers.get(id))
    }
}

pub fn builtin_editor(path: &Path) -> Option<BuiltinEditor> {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    match REGISTRY.get_or_init(Registry::with_builtins).resolve(path)? { Provider::Builtin(editor) => Some(*editor), Provider::External(_) => None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, suffixes: &[&str]) -> PluginManifest {
        PluginManifest { schema_version: 1, id: id.into(), name: "Game data".into(), suffixes: suffixes.iter().map(|s| (*s).into()).collect(), executable: "bin/server.exe".into(), args: vec!["--stdio".into()] }
    }

    #[test]
    fn builtins_and_longest_suffix_resolve_without_changing_existing_handlers() {
        let mut registry = Registry::with_builtins();
        assert_eq!(registry.resolve(Path::new("表格.XLSX")), Some(&Provider::Builtin(BuiltinEditor::Spreadsheet)));
        assert_eq!(registry.resolve(Path::new("notes.MD")), Some(&Provider::Builtin(BuiltinEditor::Markdown)));
        assert_eq!(registry.resolve(Path::new(".md")), None);
        for (name, editor) in [
            ("动画.GIF", BuiltinEditor::Image), ("photo.jpeg", BuiltinEditor::Image), ("scan.svg", BuiltinEditor::Image),
            ("voice.mp3", BuiltinEditor::Audio), ("sound.flac", BuiltinEditor::Audio),
            ("clip.mp4", BuiltinEditor::Video), ("screen.webm", BuiltinEditor::Video),
            ("角色.glb", BuiltinEditor::Model), ("mesh.stl", BuiltinEditor::Model),
        ] {
            assert_eq!(registry.resolve(Path::new(name)), Some(&Provider::Builtin(editor)), "{name}");
        }
        registry.register(manifest("example.game", &[".game", ".balance.JSON"])).unwrap();
        let plugin = registry.resolve(Path::new("角色.balance.json")).unwrap();
        assert!(matches!(plugin, Provider::External(m) if m.id == "example.game"));
        assert!(registry.resolve(Path::new("a.json")).is_none());
        assert!(matches!(registry.resolve(Path::new("save.GAME")), Some(Provider::External(_))));
    }

    #[test]
    fn discovery_reads_valid_manifests_and_isolates_broken_plugins() {
        let root = std::env::temp_dir().join(format!("markdownx-plugins-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&root);
        let good = root.join("notes");
        std::fs::create_dir_all(good.join("bin")).unwrap();
        std::fs::write(good.join(MANIFEST_FILE), serde_json::to_vec(&manifest("example.notes", &[".notes", ".NOTE"])).unwrap()).unwrap();
        let broken = root.join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join(MANIFEST_FILE), b"{not json").unwrap();
        let empty = root.join("ignored");
        std::fs::create_dir_all(&empty).unwrap();
        let found = discover(&root);
        assert_eq!(found.len(), 2);
        let good = found.iter().find(|item| item.is_ok()).unwrap().as_ref().unwrap();
        assert_eq!(good.manifest.id, "example.notes");
        assert!(found.iter().any(|item| item.is_err()));
        assert!(discover(&root.join("missing")).is_empty());
        let (registry, rejected) = Registry::with_plugins(found.into_iter().flatten());
        assert!(rejected.is_empty());
        assert!(matches!(registry.resolve(Path::new("a.note")), Some(Provider::External(manifest)) if manifest.id == "example.notes"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn registry_skips_a_conflicting_plugin_without_partial_suffixes() {
        let plugins = ["a", "b"].map(|id| {
            let suffixes = [".shared".to_string(), format!(".{id}")];
            let suffixes = suffixes.iter().map(String::as_str).collect::<Vec<_>>();
            InstalledPlugin { directory: PathBuf::from(id), manifest: manifest(id, &suffixes) }
        });
        let (registry, rejected) = Registry::with_plugins(plugins);
        assert_eq!(rejected.len(), 1);
        assert!(registry.resolve(Path::new("x.shared")).is_some());
        assert!(registry.resolve(Path::new("x.a")).is_some());
        assert!(registry.resolve(Path::new("x.b")).is_none());
        let duplicate = InstalledPlugin { directory: PathBuf::from("c"), manifest: manifest("c", &[".c", ".C"]) };
        assert!(duplicate.manifest.validate().is_err());
    }

    #[test]
    fn compound_suffixes_win_and_matching_never_uses_a_partial_name() {
        let mut registry = Registry::with_builtins();
        registry.register(manifest("example.game", &[".game", ".save.game"])).unwrap();
        assert!(matches!(registry.resolve(Path::new("存档.SAVE.GAME")), Some(Provider::External(m)) if m.id == "example.game"));
        assert!(registry.resolve(Path::new("x.game.tmp")).is_none());
        assert!(registry.resolve(Path::new("game")).is_none());
        assert_eq!(registry.external_suffixes(), vec![".game", ".save.game"]);
        assert!(!registry.external_suffixes().iter().any(|suffix| suffix == ".md"));

        // Without the shorter suffix, a compound one still has to start at a dot.
        let mut compound = Registry::with_builtins();
        compound.register(manifest("example.compound", &[".save.game"])).unwrap();
        assert!(matches!(compound.resolve(Path::new("x.save.game")), Some(Provider::External(_))));
        assert!(compound.resolve(Path::new("x.my-save.game")).is_none());
    }

    #[test]
    fn manifest_validation_rejects_bad_identity_suffixes_arguments_and_paths() {
        let valid = manifest("example.ok", &[".ok"]);
        assert!(valid.validate().is_ok());
        for mutate in [
            (|m: &mut PluginManifest| m.schema_version = 2) as fn(&mut PluginManifest),
            |m: &mut PluginManifest| m.id = "".into(),
            |m: &mut PluginManifest| m.id = "Builtin.x".into(),
            |m: &mut PluginManifest| m.id = "builtin.x".into(),
            |m: &mut PluginManifest| m.id = ".leading".into(),
            |m: &mut PluginManifest| m.id = "two..dots".into(),
            |m: &mut PluginManifest| m.id = "trailing.".into(),
            |m: &mut PluginManifest| m.id = "x".repeat(129),
            |m: &mut PluginManifest| m.name = "   ".into(),
            |m: &mut PluginManifest| m.suffixes.clear(),
            |m: &mut PluginManifest| m.suffixes = vec![".x".into(); 65],
            |m: &mut PluginManifest| m.suffixes = vec!["x".into()],
            |m: &mut PluginManifest| m.suffixes = vec![".".into()],
            |m: &mut PluginManifest| m.suffixes = vec!["..x".into()],
            |m: &mut PluginManifest| m.suffixes = vec![".x/y".into()],
            |m: &mut PluginManifest| m.suffixes = vec![".x y".into()],
            |m: &mut PluginManifest| m.suffixes = vec![format!(".{}", "x".repeat(129))],
            |m: &mut PluginManifest| m.suffixes = vec![".ok".into(), ".OK".into()],
            |m: &mut PluginManifest| m.args = vec!["x".into(); 65],
            |m: &mut PluginManifest| m.args = vec!["x".repeat(4097)],
            |m: &mut PluginManifest| m.args = vec!["a\0b".into()],
            |m: &mut PluginManifest| m.executable = "".into(),
            |m: &mut PluginManifest| m.executable = "a\0b".into(),
            |m: &mut PluginManifest| m.executable = "C:\\server.exe".into(),
            |m: &mut PluginManifest| m.executable = "/opt/server".into(),
            |m: &mut PluginManifest| m.executable = "a/../../server".into(),
            |m: &mut PluginManifest| m.executable = "a//server".into(),
            |m: &mut PluginManifest| m.executable = "a./../server".into(),
        ] {
            let mut item = valid.clone();
            mutate(&mut item);
            assert!(item.validate().is_err(), "{item:?}");
        }
    }

    #[test]
    fn discovery_ignores_unrelated_entries_sorts_and_bounds_manifests() {
        let root = std::env::temp_dir().join(format!("markdownx-plugins-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("loose.txt"), b"not a plugin").unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        for id in ["z-plugin", "a-plugin"] {
            let directory = root.join(id);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(MANIFEST_FILE), serde_json::to_vec(&manifest(id, &[&format!(".{id}")])).unwrap()).unwrap();
        }
        let oversized = root.join("oversized");
        std::fs::create_dir_all(&oversized).unwrap();
        std::fs::write(oversized.join(MANIFEST_FILE), vec![b' '; 64 * 1024 + 1]).unwrap();
        let found = discover(&root);
        assert_eq!(found.len(), 3);
        let ids = found.iter().filter_map(|item| item.as_ref().ok().map(|p| p.manifest.id.as_str())).collect::<Vec<_>>();
        assert_eq!(ids, ["a-plugin", "z-plugin"]);
        assert!(found.iter().any(|item| item.is_err()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_plugin_ids_and_suffixes_are_rejected_atomically_across_plugins() {
        let first = InstalledPlugin { directory: PathBuf::from("first"), manifest: manifest("example.same", &[".one"]) };
        let second = InstalledPlugin { directory: PathBuf::from("second"), manifest: manifest("example.same", &[".two"]) };
        let (registry, rejected) = Registry::with_plugins([first, second]);
        assert_eq!(rejected.len(), 1);
        assert!(registry.resolve(Path::new("x.one")).is_some());
        assert!(registry.resolve(Path::new("x.two")).is_none());
        assert!(registry.manifest("example.same").is_some());
    }

    #[test]
    fn registration_is_atomic_and_rejects_conflicts_and_unsafe_paths() {
        let mut registry = Registry::with_builtins();
        assert!(registry.register(manifest("example.bad", &[".custom", ".MD"])).is_err());
        assert!(registry.resolve(Path::new("file.custom")).is_none());
        for executable in ["../outside", "/tmp/server", "C:\\server.exe", "bin/../server", "bin//server", "\\\\server\\share"] {
            let mut item = manifest("example.bad", &[".custom"]);
            item.executable = executable.into();
            assert!(registry.register(item).is_err(), "{executable}");
        }
        assert!(registry.register(manifest("builtin.bad", &[".custom"])).is_err());
        assert!(registry.register(manifest("example.bad", &[".custom", ".CUSTOM"])).is_err());
        assert!(registry.register(manifest("example.good", &[".custom"])).is_ok());
        assert!(registry.register(manifest("example.good", &[".other"])).is_err());
        assert!(serde_json::from_str::<PluginManifest>(r#"{"schema_version":1,"id":"x","name":"X","suffixes":[".x"],"executable":"server","unknown":true}"#).is_err());
    }
}
