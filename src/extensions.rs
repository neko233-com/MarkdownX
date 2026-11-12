//! Installed file-format extensions. Discovery reads `plugin.json` manifests
//! from the app's plugins folder; a plugin only runs when a matching file is
//! opened, and its Markdown result is shown read-only.
use std::path::{Path, PathBuf};
use std::time::Duration;
use anyhow::{Context as _, Result, bail, ensure};
use fdl_core::extension_host::ExtensionHost;
use fdl_core::extensions::{self as core, MANIFEST_FILE, PluginManifest, Provider, Registry};
use gpui::{AppContext, Context, Window};

const START_TIMEOUT: Duration = Duration::from_secs(10);
const OPEN_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PLUGIN_FILE_BYTES: u64 = 128 * 1024 * 1024;

pub struct Plugin {
    pub directory: PathBuf,
    pub manifest: PluginManifest,
    pub enabled: bool,
    pub error: Option<String>,
}

pub struct Catalog {
    pub registry: Registry,
    pub plugins: Vec<Plugin>,
    /// Manifests that could not be read; shown in settings instead of failing startup.
    pub errors: Vec<String>,
}

/// `%LOCALAPPDATA%\markdownx\plugins` (and the same idea on macOS), or
/// `MARKDOWNX_PLUGIN_DIR` for tests and portable installs.
pub fn directory() -> PathBuf {
    if let Some(path) = std::env::var_os("MARKDOWNX_PLUGIN_DIR") { return PathBuf::from(path) }
    dirs::data_local_dir().unwrap_or_else(|| crate::store::Store::root_dir().join(".local")).join("markdownx").join("plugins")
}

/// Read the installed plugins and build the suffix registry. Disabled
/// plugins keep their manifest for the settings page but serve no suffix.
pub fn catalog() -> Catalog {
    catalog_at(&directory(), &crate::settings::prefs().disabled_plugins)
}

fn catalog_at(root: &Path, disabled: &[String]) -> Catalog {
    let mut plugins = vec![];
    let mut errors = vec![];
    let mut enabled = vec![];
    for item in core::discover(root) {
        match item {
            Ok(installed) => {
                let on = !disabled.contains(&installed.manifest.id);
                if on { enabled.push(installed.clone()); }
                plugins.push(Plugin { directory: installed.directory, manifest: installed.manifest, enabled: on, error: None });
            }
            Err(error) => errors.push(error.to_string()),
        }
    }
    let (registry, rejected) = Registry::with_plugins(enabled);
    for (path, error) in rejected {
        if let Some(plugin) = plugins.iter_mut().find(|plugin| plugin.directory == path) {
            plugin.enabled = false;
            plugin.error = Some(error);
        }
    }
    Catalog { registry, plugins, errors }
}

pub fn is_external(path: &Path) -> bool {
    matches!(catalog().registry.resolve(path), Some(Provider::External(_)))
}

pub fn suffixes() -> Vec<String> {
    catalog().registry.external_suffixes()
}

/// Open `path` through its enabled extension and return Markdown.
pub fn render(path: &Path) -> Result<String> {
    let catalog = catalog();
    let Some(Provider::External(manifest)) = catalog.registry.resolve(path) else { bail!("no extension handles this file") };
    let plugin = catalog.plugins.iter().find(|plugin| plugin.enabled && plugin.manifest.id == manifest.id).context("the extension is disabled")?;
    let mut host = ExtensionHost::start_ready(&plugin.directory, &plugin.manifest, START_TIMEOUT)?;
    let result = host.open_markdown(path, OPEN_TIMEOUT);
    host.shutdown(Duration::from_millis(500));
    result
}

/// Copy a plugin folder into the plugins directory. The source manifest is
/// validated first and an existing id is never replaced.
pub fn install(source: &Path) -> Result<String> {
    install_at(&directory(), source)
}

fn install_at(root: &Path, source: &Path) -> Result<String> {
    let manifest: PluginManifest = serde_json::from_slice(&std::fs::read(source.join(MANIFEST_FILE)).context("cannot read plugin manifest")?)
        .context("invalid plugin manifest JSON")?;
    manifest.validate()?;
    let target = root.join(&manifest.id);
    ensure!(!target.exists(), "a plugin with this id is already installed");
    std::fs::create_dir_all(root)?;
    std::fs::create_dir_all(&target)?;
    if let Err(error) = copy_tree(source, &target, 0) {
        let _ = std::fs::remove_dir_all(&target);
        return Err(error);
    }
    Ok(manifest.id)
}

fn copy_tree(source: &Path, target: &Path, depth: usize) -> Result<()> {
    ensure!(depth <= 16, "plugin folder is too deep");
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        ensure!(!file_type.is_symlink(), "plugin folder contains a symbolic link");
        let destination = target.join(entry.file_name());
        if file_type.is_dir() { std::fs::create_dir_all(&destination)?; copy_tree(&entry.path(), &destination, depth + 1)?; }
        else if file_type.is_file() {
            ensure!(entry.metadata()?.len() <= MAX_PLUGIN_FILE_BYTES, "plugin file is too large");
            std::fs::copy(entry.path(), &destination)?;
        }
    }
    Ok(())
}

impl crate::app::Workspace {
    /// Opening a file served by an external extension goes through the same
    /// save-before-replace rule as a normal document.
    pub(crate) fn open_extension_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !path.is_file() {
            self.show_toast_kind(crate::ui::ToastKind::Error, crate::settings::tr("文件不存在", "The file does not exist"), cx);
            return;
        }
        let retry = path.clone();
        if self.save_before(cx, move |w, window, cx| w.open_extension_path_now(retry, window, cx)) {
            self.open_extension_path_now(path, window, cx);
        }
    }

    fn open_extension_path_now(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.status = crate::app::SaveStatus::Idle;
        let display = path.clone();
        let task = cx.background_executor().spawn(async move { render(&path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |workspace, window, cx| match result {
                Ok(markdown) => workspace.show_extension_document(display, markdown, window, cx),
                Err(error) => workspace.show_toast_kind(crate::ui::ToastKind::Error,
                    &crate::localized_format!("扩展无法打开此文件：{}", "The extension could not open this file: {}", crate::settings::diagnostic(&error.to_string())), cx),
            });
        }).detach();
    }

    /// Show the extension's Markdown as a read-only document. It is not bound
    /// to a Markdown file, so saving never rewrites the original format.
    fn show_extension_document(&mut self, path: PathBuf, markdown: String, window: &mut Window, cx: &mut Context<Self>) {
        let (doc, _) = crate::markdown_source::parse(&path, &markdown);
        let modified = crate::app::file_modified(&path);
        let editor = cx.new(|cx| {
            let mut editor = crate::editor::view::Editor::new(doc, modified, crate::store::PageWidth::default(), window, cx);
            editor.cat_pinned = true;
            editor.set_read_only(true, cx);
            editor
        });
        self.bind_editor(editor.clone(), window, cx);
        if !self.search_open { editor.read(cx).focus(window); }
        self.editor = Some(editor);
        self.current = None;
        self.file = Some(path);
        self.file_snapshot = None;
        self.file_source = None;
        self.spreadsheet_preview = None;
        self.media_preview = None;
        self.plugin_preview = true;
        self.unsaved = false;
        self.view = crate::app::View::Doc;
        self.status = crate::app::SaveStatus::Idle;
        self.doc_menu = false;
        self.close_menus();
        self.show_toast_kind(crate::ui::ToastKind::Info, crate::settings::tr("扩展预览为只读模式", "Extension preview is read-only"), cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(id: &str, suffix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("markdownx-plugin-{}", uuid::Uuid::new_v4()));
        let directory = root.join(id);
        std::fs::create_dir_all(&directory).unwrap();
        let manifest = format!(r#"{{"schema_version":1,"id":"{id}","name":"Demo","suffixes":["{suffix}"],"executable":"server","args":[]}}"#);
        std::fs::write(directory.join(MANIFEST_FILE), manifest).unwrap();
        std::fs::write(directory.join("server"), b"plugin").unwrap();
        directory
    }

    #[test]
    fn installation_copies_once_and_refuses_invalid_or_existing_plugins() {
        let source = plugin("example.notes", ".notes");
        let root = source.parent().unwrap().join("installed");
        let id = install_at(&root, &source).unwrap();
        assert_eq!(id, "example.notes");
        assert_eq!(std::fs::read(root.join("example.notes/server")).unwrap(), b"plugin");
        assert!(install_at(&root, &source).is_err());
        std::fs::write(source.join(MANIFEST_FILE), b"{}").unwrap();
        assert!(install_at(&root, &source).is_err());
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    fn plugin_at(root: &Path, id: &str, suffixes: &[&str], executable: &str) -> PathBuf {
        let directory = root.join(id);
        std::fs::create_dir_all(&directory).unwrap();
        let manifest = serde_json::json!({"schema_version": 1, "id": id, "name": "Demo", "suffixes": suffixes, "executable": executable, "args": []});
        std::fs::write(directory.join(MANIFEST_FILE), serde_json::to_vec(&manifest).unwrap()).unwrap();
        std::fs::write(directory.join("server"), b"plugin").unwrap();
        directory
    }

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("markdownx-plugin-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn catalog_isolates_broken_manifests_and_suffix_conflicts() {
        let root = temp_root("catalog-broken");
        plugin_at(&root, "a-good", &[".good"], "server");
        plugin_at(&root, "b-builtin", &[".md"], "server");
        plugin_at(&root, "c-duplicate", &[".dup"], "server");
        plugin_at(&root, "d-duplicate", &[".dup"], "server");
        let broken = root.join("e-broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join(MANIFEST_FILE), b"{not json").unwrap();

        let catalog = catalog_at(&root, &[]);
        assert_eq!(catalog.errors.len(), 1);
        let find = |id: &str| catalog.plugins.iter().find(|plugin| plugin.manifest.id == id).unwrap();
        assert!(find("a-good").enabled && find("a-good").error.is_none());
        assert!(!find("b-builtin").enabled && find("b-builtin").error.is_some());
        assert!(!find("d-duplicate").enabled && find("d-duplicate").error.is_some());
        assert!(matches!(catalog.registry.resolve(Path::new("x.good")), Some(Provider::External(_))));
        assert!(matches!(catalog.registry.resolve(Path::new("x.dup")), Some(Provider::External(m)) if m.id == "c-duplicate"));
        assert_eq!(catalog.registry.resolve(Path::new("x.md")), Some(&Provider::Builtin(fdl_core::extensions::BuiltinEditor::Markdown)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn installation_rejects_escaping_executables_and_removes_partial_copies() {
        let root = temp_root("install-unsafe");
        let escaping = plugin_at(&root, "escaping", &[".escape"], "../outside.exe");
        assert!(install_at(&root.join("installed"), &escaping).is_err());
        assert!(!root.join("installed/escaping").exists());

        let deep = plugin_at(&root, "deep", &[".deep"], "server");
        let mut nested = deep.clone();
        for index in 0..18 {
            nested = nested.join(format!("d{index}"));
            std::fs::create_dir_all(&nested).unwrap();
        }
        std::fs::write(nested.join("file"), b"x").unwrap();
        assert!(install_at(&root.join("installed"), &deep).is_err());
        assert!(!root.join("installed/deep").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_installed_plugin_is_discovered_and_routed() {
        let root = temp_root("install-route");
        let source = plugin_at(&root, "example.route", &[".route"], "server");
        let installed = root.join("installed");
        assert_eq!(install_at(&installed, &source).unwrap(), "example.route");
        let catalog = catalog_at(&installed, &[]);
        assert!(catalog.errors.is_empty());
        assert!(matches!(catalog.registry.resolve(Path::new("文档.ROUTE")), Some(Provider::External(manifest)) if manifest.id == "example.route"));
        let disabled = catalog_at(&installed, &["example.route".to_string()]);
        assert!(disabled.registry.resolve(Path::new("文档.route")).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn disabled_plugins_keep_their_manifest_but_release_the_suffix() {
        let source = plugin("example.notes", ".notes");
        let root = source.parent().unwrap().to_path_buf();
        let catalog = catalog_at(&root, &[]);
        assert!(matches!(catalog.registry.resolve(Path::new("a.notes")), Some(Provider::External(_))));
        let disabled = catalog_at(&root, &["example.notes".to_string()]);
        assert!(!disabled.plugins[0].enabled);
        assert!(disabled.registry.resolve(Path::new("a.notes")).is_none());
        assert!(disabled.registry.resolve(Path::new("a.md")).is_some());
        let _ = std::fs::remove_dir_all(root);
    }
}
