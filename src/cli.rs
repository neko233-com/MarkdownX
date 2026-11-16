//! Headless Markdown conversion. The source and its local assets remain untouched.
use anyhow::{Context, Result, bail, ensure};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const HELP: &str = "MarkdownX CLI\n\n  markdownx convert INPUT.md [-o OUTPUT] [--to html|pdf|docx|odt|rtf|epub]\n                    [--strategy auto|builtin|pandoc] [--root DIR] [--browser PATH|--no-browser]\n                    [--pdf-engine NAME] [--reference-doc FILE] [--citeproc]\n                    [--bibliography FILE]... [--csl FILE] [--force]\n\n  auto      Built-in HTML; for PDF use a local Chromium browser, then Pandoc; other formats use Pandoc.\n            With --citeproc, use Pandoc HTML and print PDF through Chromium when available.\n  builtin   MarkdownX renderer; PDF requires Edge, Chrome or --browser.\n  pandoc    Use Pandoc; Windows downloads verified tools on first use if needed.\n  --pdf-engine    Choose a Pandoc PDF engine, e.g. typst or xelatex (auto or pandoc strategy).\n  --no-browser    Use Pandoc fallback for auto PDF even if a browser exists.\n  --reference-doc Use a DOCX/ODT style template for the matching office format.\n  --citeproc      Resolve Pandoc citations; --bibliography and --csl select local files.\n  --root          Built-in asset boundary; Pandoc's secondary resource directory.\n  --force         Replace an existing output file.\n  Set MARKDOWNX_FORMATS_OFFLINE=1 to disable Windows tool downloads.\n";

struct Request {
    input: PathBuf,
    output: Option<PathBuf>,
    format: Option<String>,
    strategy: String,
    root: Option<PathBuf>,
    browser: Option<PathBuf>,
    no_browser: bool,
    pdf_engine: Option<OsString>,
    reference_doc: Option<PathBuf>,
    citeproc: bool,
    bibliography: Vec<PathBuf>,
    csl: Option<PathBuf>,
    force: bool,
}

/// Return None for ordinary GUI launches; otherwise return the process exit code.
pub(crate) fn dispatch(args: &[OsString]) -> Option<i32> {
    let first = args.first()?.to_str()?;
    if !matches!(first, "convert" | "export" | "--help" | "-h" | "--version" | "-V") { return None; }
    #[cfg(target_os = "windows")]
    unsafe { let _ = windows::Win32::System::Console::AttachConsole(windows::Win32::System::Console::ATTACH_PARENT_PROCESS); }
    if matches!(first, "--version" | "-V") { println!("MarkdownX {}", env!("CARGO_PKG_VERSION")); return Some(0); }
    if matches!(first, "--help" | "-h") || args.iter().skip(1).any(|a| a == "--help" || a == "-h") { print!("{HELP}"); return Some(0); }
    match parse(&args[1..]).and_then(convert) {
        Ok(path) => {
            let value = path.to_string_lossy();
            let display = value.strip_prefix(r"\\?\UNC\").map(|s| format!(r"\\{s}"))
                .or_else(|| value.strip_prefix(r"\\?\").map(str::to_string)).unwrap_or_else(|| value.into_owned());
            println!("{display}"); Some(0)
        }
        Err(error) => { eprintln!("MarkdownX: {error:#}"); Some(2) }
    }
}

fn parse(args: &[OsString]) -> Result<Request> {
    let mut request = Request { input: PathBuf::new(), output: None, format: None, strategy: "auto".into(), root: None, browser: None, no_browser: false, pdf_engine: None, reference_doc: None, citeproc: false, bibliography: vec![], csl: None, force: false };
    let mut at = 0;
    while at < args.len() {
        let arg = &args[at];
        let flag = arg.to_str().unwrap_or("");
        if flag == "--force" { request.force = true; at += 1; continue; }
        if flag == "--no-browser" { request.no_browser = true; at += 1; continue; }
        if flag == "--citeproc" { request.citeproc = true; at += 1; continue; }
        if matches!(flag, "-o" | "--output" | "--to" | "--format" | "--strategy" | "--root" | "--browser" | "--pdf-engine" | "--reference-doc" | "--bibliography" | "--csl") {
            let value = args.get(at + 1).with_context(|| format!("missing value for {flag}"))?;
            match flag {
                "-o" | "--output" => request.output = Some(PathBuf::from(value)),
                "--to" | "--format" => request.format = Some(value.to_str().context("format must be UTF-8")?.to_ascii_lowercase()),
                "--strategy" => request.strategy = value.to_str().context("strategy must be UTF-8")?.to_ascii_lowercase(),
                "--root" => request.root = Some(PathBuf::from(value)),
                "--pdf-engine" => request.pdf_engine = Some(value.clone()),
                "--reference-doc" => request.reference_doc = Some(PathBuf::from(value)),
                "--bibliography" => request.bibliography.push(PathBuf::from(value)),
                "--csl" => request.csl = Some(PathBuf::from(value)),
                _ => request.browser = Some(PathBuf::from(value)),
            }
            at += 2;
            continue;
        }
        ensure!(!flag.starts_with('-'), "unknown option: {flag}");
        ensure!(request.input.as_os_str().is_empty(), "only one input file is supported");
        request.input = PathBuf::from(arg);
        at += 1;
    }
    ensure!(!request.input.as_os_str().is_empty(), "missing Markdown input; run markdownx convert --help");
    ensure!(matches!(request.strategy.as_str(), "auto" | "builtin" | "pandoc"), "strategy must be auto, builtin or pandoc");
    Ok(request)
}

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!("markdownx-convert-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for TempDir { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

fn convert(request: Request) -> Result<PathBuf> {
    let input = request.input.canonicalize().context("input Markdown file is unavailable")?;
    ensure!(input.is_file(), "input is not a regular file");
    ensure!(input.extension().and_then(OsStr::to_str).is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")), "input must be a Markdown file");
    let root = request.root.as_deref().unwrap_or_else(|| input.parent().unwrap_or(Path::new("."))).canonicalize().context("asset root is unavailable")?;
    ensure!(root.is_dir() && input.starts_with(&root), "input is outside the asset root");
    let format = request.format.or_else(|| request.output.as_ref().and_then(|p| p.extension()).and_then(OsStr::to_str).map(str::to_ascii_lowercase))
        .context("specify --to html, pdf, docx, odt, rtf or epub, or use an output filename with that extension")?;
    ensure!(matches!(format.as_str(), "html" | "pdf" | "docx" | "odt" | "rtf" | "epub"), "format must be html, pdf, docx, odt, rtf or epub");
    let pandoc_required = matches!(format.as_str(), "docx" | "odt" | "rtf" | "epub");
    ensure!(!pandoc_required || request.strategy != "builtin", "this format requires Pandoc; use --strategy auto or pandoc");
    ensure!(!request.citeproc || request.strategy != "builtin", "--citeproc requires the auto or Pandoc strategy");
    ensure!(request.citeproc || (request.bibliography.is_empty() && request.csl.is_none()), "--bibliography and --csl require --citeproc");
    ensure!(request.reference_doc.is_none() || matches!(format.as_str(), "docx" | "odt"), "--reference-doc requires DOCX or ODT output");
    let reference_doc = request.reference_doc.as_ref().map(|path| -> Result<PathBuf> {
        let path = path.canonicalize().context("reference document is unavailable")?;
        ensure!(path.is_file() && path.extension().and_then(OsStr::to_str).is_some_and(|ext| ext.eq_ignore_ascii_case(&format)), "reference document must match the output format");
        Ok(path)
    }).transpose()?;
    let bibliography = request.bibliography.iter().map(|path| canonical_file(path, "bibliography")).collect::<Result<Vec<_>>>()?;
    let csl = request.csl.as_ref().map(|path| canonical_file(path, "CSL style")).transpose()?;
    ensure!(request.pdf_engine.is_none() || (request.strategy != "builtin" && format == "pdf"), "--pdf-engine requires the auto or Pandoc strategy and PDF output");
    ensure!(request.browser.is_none() || (request.strategy != "pandoc" && format == "pdf" && request.pdf_engine.is_none()), "--browser requires PDF output with auto or builtin strategy and no --pdf-engine");
    ensure!(!request.no_browser || (format == "pdf" && request.strategy != "builtin" && request.browser.is_none()), "--no-browser requires PDF output and cannot be combined with --browser or the builtin strategy");
    let output = request.output.unwrap_or_else(|| input.with_extension(&format));
    let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).context("could not create output directory")?;
    let parent = parent.canonicalize().context("output directory is unavailable")?;
    let output = parent.join(output.file_name().context("output has no filename")?);
    if output.exists() {
        ensure!(output.canonicalize()? != input, "output must not replace the input");
        ensure!(request.force, "output already exists; use --force to replace it");
        let meta = std::fs::symlink_metadata(&output)?;
        #[cfg(windows)]
        let reparse = { use std::os::windows::fs::MetadataExt; meta.file_attributes() & 0x400 != 0 };
        #[cfg(not(windows))]
        let reparse = false;
        ensure!(meta.is_file() && !meta.file_type().is_symlink() && !reparse, "output is not a regular file");
    }
    let temp = TempDir::new()?;
    let rendered = temp.0.join(format!("output.{format}"));
    if request.strategy == "pandoc" || pandoc_required || request.pdf_engine.is_some() || (format == "html" && request.citeproc) {
        let options = PandocOptions { format: &format, engine: request.pdf_engine.as_deref(), reference: reference_doc.as_deref(), citeproc: request.citeproc, bibliography: &bibliography, csl: csl.as_deref() };
        run_pandoc(&input, &root, &rendered, &options, &temp.0)?;
        if format == "html" {
            let html = String::from_utf8(crate::store::read_bounded(&rendered, 256 * 1024 * 1024)?).context("Pandoc HTML is not UTF-8")?;
            std::fs::write(&rendered, rebase_html_links(&html, &input, &root, &parent)?)?;
        }
    } else {
        let mut html = if request.citeproc {
            let cited = temp.0.join("cited.html");
            let options = PandocOptions { format: "html", engine: None, reference: None, citeproc: true, bibliography: &bibliography, csl: csl.as_deref() };
            run_pandoc(&input, &root, &cited, &options, &temp.0)?;
            let html = String::from_utf8(crate::store::read_bounded(&cited, 256 * 1024 * 1024).context("Pandoc did not produce HTML")?).context("Pandoc HTML is not UTF-8")?;
            rebase_html_links(&html, &input, &root, &parent)?
        } else { render_builtin(&input, &root, &parent)? };
        if format == "html" { std::fs::write(&rendered, html)?; }
        else {
            let base = url::Url::from_directory_path(&parent).map_err(|_| anyhow::anyhow!("cannot create an output directory URL"))?;
            html = html.replacen("<head>", &format!("<head><base href=\"{}\">", base.as_str().replace('&', "&amp;").replace('"', "&quot;")), 1);
            let browsers = if request.no_browser { vec![] } else { request.browser.clone().map(|path| vec![path]).unwrap_or_else(find_browsers) };
            let mut problems = Vec::new();
            let mut completed = false;
            for browser in browsers {
                let _ = std::fs::remove_file(&rendered);
                match run_browser(&browser, &html, &rendered, &temp.0) {
                    Ok(()) if is_pdf(&rendered) => { completed = true; break; }
                    Ok(()) => problems.push(format!("{} did not produce a PDF", browser.display())),
                    Err(error) => problems.push(format!("{}: {error:#}", browser.display())),
                }
            }
            if !completed && request.strategy == "auto" && request.browser.is_none() {
                let _ = std::fs::remove_file(&rendered);
                let engine = preferred_pandoc_engine()?;
                let options = PandocOptions { format: &format, engine, reference: None, citeproc: request.citeproc, bibliography: &bibliography, csl: csl.as_deref() };
                match run_pandoc(&input, &root, &rendered, &options, &temp.0) {
                    Ok(()) => completed = true,
                    Err(error) => {
                        problems.push(format!("Pandoc: {error:#}"));
                        let retry_typst = if engine.is_some() { false } else if typst_available() { true } else {
                            #[cfg(target_os = "windows")]
                            { match typst_path() { Ok(_) => true, Err(error) => { problems.push(format!("Typst: {error:#}")); false } } }
                            #[cfg(not(target_os = "windows"))]
                            { false }
                        };
                        if retry_typst {
                            let _ = std::fs::remove_file(&rendered);
                            let options = PandocOptions { engine: Some(OsStr::new("typst")), ..options };
                            match run_pandoc(&input, &root, &rendered, &options, &temp.0) {
                                Ok(()) => completed = true,
                                Err(error) => problems.push(format!("Pandoc with Typst: {error:#}")),
                            }
                        }
                    }
                }
            }
            if !completed { bail!("PDF conversion failed: {}", if problems.is_empty() { "no Chromium browser found; use --browser PATH or --strategy pandoc".into() } else { problems.join("; ") }); }
        }
    }
    let bytes = crate::store::read_bounded(&rendered, 256 * 1024 * 1024).context("conversion did not produce an output")?;
    ensure!(!bytes.is_empty(), "conversion produced an empty output");
    if format == "pdf" { ensure!(bytes.starts_with(b"%PDF-"), "converter did not produce a PDF"); }
    if pandoc_required { validate_pandoc_output(&bytes, &format)?; }
    if request.force { crate::workspace_fs::write_atomic(&output, &bytes)?; }
    else {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&output).context("could not create output")?;
        if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            drop(file); let _ = std::fs::remove_file(&output); return Err(error.into());
        }
    }
    Ok(output)
}

fn render_builtin(input: &Path, root: &Path, output_dir: &Path) -> Result<String> {
    let bytes = crate::store::read_bounded(input, 16 * 1024 * 1024).context("could not read Markdown")?;
    let source = std::str::from_utf8(&bytes).context("Markdown must be UTF-8")?;
    let footnotes = crate::footnotes::prepare(source);
    let (doc, _) = crate::markdown_source::parse_for_book(input, &footnotes.source, root);
    for asset in doc.asset_sources() {
        let lower = asset.to_ascii_lowercase();
        if asset.is_empty() || asset.starts_with('#') || lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("data:") { continue; }
        ensure!(crate::store::export_path_in(root, &asset).is_some(), "local asset is missing or outside the asset root: {asset}");
    }
    let packed = fdl_core::book::pack(vec![fdl_core::book::PackInput::new(doc).with_asset_base(input.parent().context("input has no parent")?.to_path_buf())],
        &fdl_core::book::PackOptions { assets_root: Some(root.to_path_buf()), ..Default::default() })?;
    let mut doc = crate::docxml::parse(&packed.chapters[0].content)?;
    inline_rich_assets(&mut doc.title, &packed)?;
    inline_book_assets(&mut doc.blocks, &packed)?;
    let mut local_links = std::collections::HashSet::new();
    let base = url::Url::from_directory_path(output_dir).map_err(|_| anyhow::anyhow!("cannot create an output directory URL"))?;
    rebase_rich_links(&mut doc.title, input, root, &base, &mut local_links)?;
    rebase_links(&mut doc.blocks, input, root, output_dir, &mut local_links)?;
    let anchors = footnotes.export_anchors(&doc);
    footnotes.render(crate::html::to_html_with_anchors(&doc, &crate::html::Options { sub_pages: &[], asset_root: Some(root) }, anchors, &local_links))
}

fn rebase_links(blocks: &mut [crate::model::Block], input: &Path, root: &Path, output_dir: &Path, local_links: &mut std::collections::HashSet<String>) -> Result<()> {
    let base = url::Url::from_directory_path(output_dir).map_err(|_| anyhow::anyhow!("cannot create an output directory URL"))?;
    for block in blocks {
        rebase_rich_links(&mut block.text, input, root, &base, local_links)?;
        if let crate::model::BlockKind::Table(table) = &mut block.kind {
            for cell in table.rows.iter_mut().flatten() { rebase_links(&mut cell.blocks, input, root, output_dir, local_links)?; }
        }
        rebase_links(&mut block.children, input, root, output_dir, local_links)?;
    }
    Ok(())
}

fn rebase_rich_links(text: &mut crate::model::RichText, input: &Path, root: &Path, base: &url::Url, local_links: &mut std::collections::HashSet<String>) -> Result<()> {
    for run in &mut text.runs {
        if let Some(link) = &mut run.marks.link && let Some(rebased) = rebase_link(link, input, root, base, true)? {
            if rebased.starts_with("file:") { local_links.insert(rebased.clone()); }
            *link = rebased;
        }
    }
    Ok(())
}

fn rebase_html_links(html: &str, input: &Path, root: &Path, output_dir: &Path) -> Result<String> {
    let base = url::Url::from_directory_path(output_dir).map_err(|_| anyhow::anyhow!("cannot create an output directory URL"))?;
    fdl_core::html_assets::rewrite_links(html, |link| rebase_link(link, input, root, &base, true))
}

fn rebase_link(link: &str, input: &Path, root: &Path, base: &url::Url, allow_file_url: bool) -> Result<Option<String>> {
    let Some((path, fragment)) = crate::paste::split_relative_link(link) else { return Ok(None) };
    if path.is_empty() { return Ok(None); }
    let target = crate::workspace_files::resolve_relative_link_for_export(root, input, path)
        .with_context(|| format!("cannot safely resolve local link {link}; use --root to include its directory"))?;
    let is_directory = target.is_dir() || path.ends_with('/');
    let target = if is_directory { url::Url::from_directory_path(target) } else { url::Url::from_file_path(target) }
        .map_err(|_| anyhow::anyhow!("cannot create a link URL"))?;
    let fragment = fragment.map(|fragment| format!("#{fragment}")).unwrap_or_default();
    let relative = base.make_relative(&target);
    if relative.as_deref().is_none_or(|path| !crate::paste::relative_link_ok(path)) {
        ensure!(allow_file_url, "linked file is on a different volume from the output: {link}");
        return Ok(Some(format!("{target}{fragment}")));
    }
    let relative = relative.expect("validated relative link");
    Ok(Some(format!("{}{fragment}", if relative.starts_with('.') { relative } else { format!("./{relative}") })))
}

fn inline_rich_assets(text: &mut crate::model::RichText, book: &fdl_core::book::Book) -> Result<()> {
    for run in &mut text.runs {
        if let Some(inline) = &mut run.marks.inline && let crate::model::InlineKind::Image(image) = &inline.kind
            && let Some(uri) = book.data_uri(image.src.trim_start_matches("./")) {
            let mut updated = image.clone(); updated.src = uri;
            crate::docxml::remap_inline_image(inline, updated)?;
        }
    }
    Ok(())
}

fn inline_book_assets(blocks: &mut [crate::model::Block], book: &fdl_core::book::Book) -> Result<()> {
    use crate::model::{BlockKind, InteractiveKind};
    for block in blocks {
        if let BlockKind::Image { src, .. } = &mut block.kind
            && let Some(uri) = book.data_uri(src.trim_start_matches("./")) { *src = uri; }
        inline_rich_assets(&mut block.text, book)?;
        if let BlockKind::Interactive(widget) = &mut block.kind && widget.kind == InteractiveKind::Html {
            widget.source = fdl_core::html_assets::rewrite_references(&widget.source, |reference| {
                let script = reference.script_parts();
                let (src, fragment) = reference.media_time().or(script).unwrap_or((&reference.value, ""));
                let fragment = if script.is_some() { if reference.module { fdl_core::html_assets::script_identity(fragment) } else { String::new() } } else { fragment.to_string() };
                Ok::<_, anyhow::Error>(book.data_uri(src.trim_start_matches("./")).map(|uri| format!("{uri}{fragment}")))
            })?;
        }
        if let BlockKind::Table(table) = &mut block.kind {
            for cell in table.rows.iter_mut().flatten() { inline_book_assets(&mut cell.blocks, book)?; }
        }
        inline_book_assets(&mut block.children, book)?;
    }
    Ok(())
}

fn find_program(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(name)).find(|path| path.is_file())
}

fn absolute_program(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() { Ok(path) } else { Ok(std::env::current_dir()?.join(path)) }
}

fn pandoc_path() -> Result<PathBuf> {
    let name = if cfg!(windows) { "pandoc.exe" } else { "pandoc" };
    if let Some(path) = find_program(name) { return absolute_program(path); }
    #[cfg(target_os = "windows")]
    { absolute_program(crate::format_tools::install(crate::format_tools::Tool::Pandoc)?) }
    #[cfg(not(target_os = "windows"))]
    { bail!("Pandoc is not installed or not on PATH") }
}

fn typst_path() -> Result<PathBuf> {
    let name = if cfg!(windows) { "typst.exe" } else { "typst" };
    if let Some(path) = find_program(name) { return absolute_program(path); }
    #[cfg(target_os = "windows")]
    { absolute_program(crate::format_tools::install(crate::format_tools::Tool::Typst)?) }
    #[cfg(not(target_os = "windows"))]
    { bail!("Typst is not installed or not on PATH") }
}

fn preferred_pandoc_engine() -> Result<Option<&'static OsStr>> {
    let latex = if cfg!(windows) { "pdflatex.exe" } else { "pdflatex" };
    if find_program(latex).is_some() { return Ok(None); }
    #[cfg(target_os = "windows")]
    { typst_path()?; Ok(Some(OsStr::new("typst"))) }
    #[cfg(not(target_os = "windows"))]
    { Ok(typst_available().then_some(OsStr::new("typst"))) }
}

fn typst_available() -> bool {
    if find_program(if cfg!(windows) { "typst.exe" } else { "typst" }).is_some() { return true; }
    #[cfg(target_os = "windows")]
    { crate::format_tools::cached(crate::format_tools::Tool::Typst).is_some() }
    #[cfg(not(target_os = "windows"))]
    { false }
}

fn is_pdf(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 5];
    std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut magic)).is_ok() && &magic == b"%PDF-"
}

fn find_browsers() -> Vec<PathBuf> {
    let mut browsers = vec![];
    #[cfg(target_os = "windows")]
    for path in [
        std::env::var_os("PROGRAMFILES(X86)").map(|p| PathBuf::from(p).join("Microsoft/Edge/Application/msedge.exe")),
        std::env::var_os("PROGRAMFILES").map(|p| PathBuf::from(p).join("Google/Chrome/Application/chrome.exe")),
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Google/Chrome/Application/chrome.exe")),
    ].into_iter().flatten() { if path.is_file() && !browsers.contains(&path) { browsers.push(path); } }
    #[cfg(target_os = "macos")]
    for path in ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"] {
        if Path::new(path).is_file() { browsers.push(PathBuf::from(path)); }
    }
    for name in if cfg!(windows) { &["msedge.exe", "chrome.exe", "chromium.exe"][..] } else { &["chromium", "chromium-browser", "google-chrome", "microsoft-edge"][..] } {
        if let Some(path) = find_program(name) && !browsers.contains(&path) { browsers.push(path); }
    }
    browsers
}

fn run_browser(browser: &Path, html: &str, output: &Path, temp: &Path) -> Result<()> {
    ensure!(browser.is_file(), "browser executable is unavailable: {}", browser.display());
    let page = temp.join("input.html");
    std::fs::write(&page, html)?;
    let page_url = url::Url::from_file_path(&page).map_err(|_| anyhow::anyhow!("cannot create a local page URL"))?;
    let mut command = Command::new(browser);
    command.args(["--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check", "--disable-extensions", "--disable-background-networking", "--disable-sync", "--no-pdf-header-footer", "--virtual-time-budget=5000"])
        .arg(format!("--user-data-dir={}", temp.join(format!("browser-profile-{}", uuid::Uuid::new_v4())).display()))
        .arg(format!("--print-to-pdf={}", output.display())).arg(page_url.as_str());
    run_command(&mut command, temp)
}

fn canonical_file(path: &Path, kind: &str) -> Result<PathBuf> {
    let resolved = path.canonicalize().with_context(|| format!("{kind} file is unavailable"))?;
    ensure!(resolved.is_file(), "{kind} is not a regular file");
    Ok(resolved)
}

struct PandocOptions<'a> { format: &'a str, engine: Option<&'a OsStr>, reference: Option<&'a Path>, citeproc: bool, bibliography: &'a [PathBuf], csl: Option<&'a Path> }

fn run_pandoc(input: &Path, root: &Path, output: &Path, options: &PandocOptions, temp: &Path) -> Result<()> {
    let pandoc = pandoc_path()?;
    let mut command = Command::new(pandoc);
    let paths = std::env::join_paths([input.parent().context("input has no parent")?, root]).context("invalid resource directory")?;
    command.arg(input).arg("--standalone").arg("--resource-path").arg(paths).arg("-o").arg(output);
    if options.format == "html" { command.arg("--embed-resources"); }
    if let Some(engine) = options.engine {
        command.arg("--pdf-engine");
        let path = Path::new(engine);
        let typst_name = path.file_name().and_then(OsStr::to_str).is_some_and(|name| name.eq_ignore_ascii_case("typst") || name.eq_ignore_ascii_case("typst.exe"));
        if typst_name {
            let typst = if path.components().count() == 1 { typst_path()? } else { absolute_program(path.to_path_buf())? };
            ensure!(typst.is_file(), "Typst executable is unavailable: {}", typst.display());
            let directory = typst.parent().context("Typst has no parent directory")?;
            let original = std::env::var_os("PATH").unwrap_or_default();
            let paths = std::env::join_paths(std::iter::once(directory.to_path_buf()).chain(std::env::split_paths(&original)))?;
            // Pandoc recognizes exactly "typst" when preparing image resources.
            command.env("PATH", paths).arg("typst");
        } else { command.arg(engine); }
    }
    if let Some(reference) = options.reference { command.arg("--reference-doc").arg(reference); }
    if options.citeproc { command.args(["--citeproc", "--fail-if-warnings"]); }
    for path in options.bibliography { command.arg("--bibliography").arg(path); }
    if let Some(path) = options.csl { command.arg("--csl").arg(path); }
    command.current_dir(input.parent().context("input has no parent")?);
    run_command(&mut command, temp)
}

fn validate_pandoc_output(bytes: &[u8], format: &str) -> Result<()> {
    use std::io::Read;
    if format == "rtf" { ensure!(bytes.starts_with(b"{\\rtf"), "converter did not produce an RTF document"); return Ok(()); }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("converter did not produce a valid office document")?;
    if format == "docx" {
        archive.by_name("[Content_Types].xml").context("DOCX content types are missing")?;
        archive.by_name("word/document.xml").context("DOCX document body is missing")?;
    } else {
        let mut mime = String::new();
        archive.by_name("mimetype").context("document media type is missing")?.take(128).read_to_string(&mut mime)?;
        if format == "odt" {
            ensure!(mime == "application/vnd.oasis.opendocument.text", "invalid ODT media type");
            archive.by_name("content.xml").context("ODT document body is missing")?;
        } else {
            ensure!(mime == "application/epub+zip", "invalid EPUB media type");
            archive.by_name("META-INF/container.xml").context("EPUB container is missing")?;
        }
    }
    Ok(())
}

fn run_command(command: &mut Command, temp: &Path) -> Result<()> {
    let stderr = temp.join("converter.stderr");
    let log = std::fs::File::create(&stderr)?;
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::from(log));
    #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let mut child = command.spawn().context("could not start converter")?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? { break status; }
        if start.elapsed() > Duration::from_secs(120) { let _ = child.kill(); let _ = child.wait(); bail!("converter timed out"); }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        let message = crate::store::read_bounded(&stderr, 1024 * 1024).unwrap_or_default();
        bail!("converter failed ({status}): {}", String::from_utf8_lossy(&message).trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_html_embeds_relative_images_and_preserves_files() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("shots")).unwrap();
        let image = root.join("shots/图 1.svg");
        std::fs::write(&image, b"<svg xmlns='http://www.w3.org/2000/svg' width='20' height='10'></svg>").unwrap();
        let input = root.join("story.md");
        let original = "# Story\n\n| Shot | Frame |\n| --- | --- |\n| 1 | ![A](./shots/%E5%9B%BE%201.svg) |\n| 2 | ![B](shots/%E5%9B%BE%201.svg) |\n";
        std::fs::write(&input, original).unwrap();
        let output = root.join("story.html");
        let make = |force| Request { input: input.clone(), output: Some(output.clone()), format: None, strategy: "builtin".into(), root: None, browser: None, no_browser: false, pdf_engine: None, reference_doc: None, citeproc: false, bibliography: vec![], csl: None, force };
        convert(make(false)).unwrap();
        let html = std::fs::read_to_string(&output).unwrap();
        assert!(html.contains("<table") && html.matches("data:image/svg+xml;base64,").count() == 2, "{html}");
        assert_eq!(std::fs::read_to_string(&input).unwrap(), original);
        assert!(convert(make(false)).is_err());
        std::fs::remove_file(&image).unwrap();
        assert!(convert(make(true)).is_err());
        assert_eq!(std::fs::read_to_string(&output).unwrap(), html);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builtin_html_embeds_svg_view_fragments() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-svg-view-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='100' height='50' viewBox='0 0 100 50'><view id='right' viewBox='50 0 50 50'/><rect width='50' height='50' fill='red'/><rect x='50' width='50' height='50' fill='blue'/></svg>";
        std::fs::write(root.join("frame#take.svg"), svg).unwrap();
        let input = root.join("story.md");
        let source = "# Story\n\n![frame](./frame%23take.svg#right)\n\n| A | B |\n|---|---|\n| 1 | ![frame](<frame%23take.svg#svgView(viewBox(50,0,50,50))>) |\n";
        std::fs::write(&input, source).unwrap();
        let html = render_builtin(&input, &root, &root).unwrap();
        assert_eq!(html.matches("data:image/svg+xml;base64,").count(), 2);
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        let outside = root.with_extension("svg");
        std::fs::write(&outside, svg).unwrap();
        let escaping = format!("# Story\n\n![outside](../{}#right)\n", outside.file_name().unwrap().to_string_lossy());
        std::fs::write(&input, escaping).unwrap();
        assert!(render_builtin(&input, &root, &root).is_err());
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builtin_html_preserves_rich_title_and_embeds_its_image() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-title-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("shot.svg"), "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'/>").unwrap();
        let input = root.join("story.md");
        let source = "# Story ![frame](./shot.svg) $\\alpha$\n\n[title](#story-frame-alpha)\n";
        std::fs::write(&input, source).unwrap();
        let html = render_builtin(&input, &root, &root).unwrap();
        assert!(html.contains("id=\"story-frame-alpha\"") && html.contains("data:image/svg+xml;base64,") && html.contains("<math"), "{html}");
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builtin_html_embeds_local_interactive_dependencies() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-widget-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("game.js"), "document.body.dataset.ready='yes';").unwrap();
        let input = root.join("game.md");
        std::fs::write(&input, "# Game\n\n<div class='game'><div><canvas></canvas></div></div>\n<script src='./game.js'></script>\n").unwrap();
        let html = render_builtin(&input, &root, &root).unwrap();
        assert!(html.contains("data:text/javascript;base64,") || html.contains("data:application/javascript;base64,"), "{html}");
        assert!(html.contains("connect-src data:"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builtin_html_renders_multiline_display_math() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-display-math-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let input = root.join("paper.md");
        let source = "# Paper\n\nBefore $\\alpha$ after.\n\n$$\n\\int_0^1 \\frac{x^2}{1+x} \\, dx\n$$\n";
        std::fs::write(&input, source).unwrap();
        let html = render_builtin(&input, &root, &root).unwrap();
        assert_eq!(html.matches("<math ").count(), 2, "{html}");
        assert!(html.contains("class=\"mathb\"") && html.contains("<mfrac>"), "{html}");
        assert!(!html.contains("$$"), "{html}");
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builtin_html_preserves_script_query_module_identity() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-script-query-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("game.mjs"), "globalThis.count=(globalThis.count||0)+1;").unwrap();
        let input = root.join("game.md");
        let source = "# Game\n\n<canvas></canvas>\n<script type='module' src='./game.mjs?v=1'></script>\n";
        std::fs::write(&input, source).unwrap();
        let html = render_builtin(&input.canonicalize().unwrap(), &root.canonicalize().unwrap(), &root).unwrap();
        assert!(html.contains("data:text/javascript;base64,") && html.contains("#%3F%76%3D%31"), "{html}");
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builtin_html_embeds_timed_video_from_canonical_windows_path() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-timed-media-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("clip#take.mp4"), b"video").unwrap();
        let input = root.join("story.md");
        let source = "# Story\n\n<video src='./clip%23take.mp4?v=1#t=1,2'></video>\n";
        std::fs::write(&input, source).unwrap();
        let canonical_root = root.canonicalize().unwrap();
        let html = render_builtin(&input.canonicalize().unwrap(), &canonical_root, &canonical_root).unwrap();
        assert!(html.contains("data:video/mp4;base64,") && html.contains("#t=1,2"), "{html}");
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builtin_html_embeds_module_graph_and_preserves_source() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-module-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("helper.mjs"), "export const n=42;").unwrap();
        std::fs::write(root.join("frame.svg"), "<svg/>").unwrap();
        std::fs::write(root.join("game.mjs"), "import {n} from './helper.mjs'; const art=new URL('./frame.svg',import.meta.url); console.log(n);").unwrap();
        let input = root.join("game.md");
        let source = "# Game\n\n<canvas></canvas>\n<script type='module' src='game.mjs'></script>\n";
        std::fs::write(&input, source).unwrap();
        let html = render_builtin(&input, &root, &root).unwrap();
        assert!(html.contains("data:text/javascript;base64,"), "{html}");
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builtin_footnotes_preserve_source_and_render_supported_content() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-footnote-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("note.svg"), "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'/>").unwrap();
        let input = root.join("paper.md");
        let source = "# Paper\n\nFirst[^a] and again[^a], second[^b], unresolved[^missing].\n\n`[^a]` and \\[^a].\n\n[^a]: A **bold** note.\n\n    More ![image](note.svg).\n\n[^b]: Second note.\n\n```text\n[^a]\n```\n";
        std::fs::write(&input, source).unwrap();
        let html = render_builtin(&input, &root, &root).unwrap();
        assert_eq!(html.matches("class=\"footnote-ref\"").count(), 3, "{html}");
        assert_eq!(html.matches("class=\"b footnote\"").count(), 2, "{html}");
        assert!(html.contains("<b>bold</b>") && html.contains("data:image/svg+xml;base64,") && html.contains("[^missing]"), "{html}");
        assert!(!html.contains("MDXFOOTNOTE") || html.contains("href=\"#MDXFOOTNOTE"), "{html}");
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builtin_footnotes_follow_display_order_when_definitions_are_interleaved() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-footnote-order-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let input = root.join("paper.md");
        let source = "# Paper\n\nFirst[^a].\n\n[^a]: nested[^c] and repeat[^b]\n\nLater[^b] and again[^a].\n\n[^b]: back[^a]\n\n[^c]: Last.\n";
        std::fs::write(&input, source).unwrap();
        let html = render_builtin(&input, &root, &root).unwrap();
        let numbers: Vec<_> = html.split("<sup class=\"footnote-ref\">").skip(1)
            .map(|s| s.split("</a>").next().unwrap().rsplit('>').next().unwrap()).collect();
        assert_eq!(numbers, ["1", "2", "1", "3", "2", "1"], "{html}");
        assert_eq!(html.matches("class=\"b footnote\"").count(), 3, "{html}");
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builtin_footnotes_keep_original_heading_fragments() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-footnote-anchors-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let input = root.join("paper.md");
        let source = "# Paper[^a]\n\n[First](#sectiona) [Second](#sectiona-1)\n\n## Section[^a]\n\n## Section[^a]\n\n[^a]: Note.\n";
        std::fs::write(&input, source).unwrap();
        let html = render_builtin(&input, &root, &root).unwrap();
        assert!(html.contains("<title>Paper1</title>"), "{html}");
        assert!(html.contains("class=\"title\" id=\"papera\""), "{html}");
        assert!(html.contains("class=\"b h h2\" id=\"sectiona\""), "{html}");
        assert!(html.contains("class=\"b h h2\" id=\"sectiona-1\""), "{html}");
        assert!(html.contains("href=\"#sectiona\"") && html.contains("href=\"#sectiona-1\""), "{html}");
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_rebases_missing_links_to_the_source_directory() {
        let root = std::env::temp_dir().join(format!("markdownx-cli-missing-link-{}", uuid::Uuid::new_v4()));
        let output = root.join("export");
        std::fs::create_dir_all(&output).unwrap();
        let input = root.join("paper.md");
        let source = "# Paper\n\n[Draft](./future%20%231.pdf#page=2)\n\n[Reference][p]\n\n[p]: future%20%231.pdf#page=2\n";
        std::fs::write(&input, source).unwrap();
        std::fs::write(output.join("future #1.pdf"), b"wrong file").unwrap();
        let html = render_builtin(&input, &root, &output).unwrap();
        assert_eq!(html.matches("href=\"../future%20%231.pdf#page=2\"").count(), 2, "{html}");
        let raw = "<p><a href=\"./future%20%231.pdf#page=2\">Draft</a></p>";
        assert!(rebase_html_links(raw, &input, &root, &output).unwrap().contains("href=\"../future%20%231.pdf#page=2\""));
        let base = url::Url::from_directory_path(&output).unwrap();
        assert!(rebase_link("../outside.pdf", &input, &root, &base, true).is_err());
        assert_eq!(rebase_link("./", &input, &root, &base, true).unwrap().as_deref(), Some("../"));
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn document_validation_requires_the_right_file_structure() {
        use std::io::Write;
        fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (name, content) in entries {
                writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                writer.write_all(content).unwrap();
            }
            writer.finish().unwrap().into_inner()
        }
        assert!(validate_pandoc_output(b"not an archive", "docx").is_err());
        assert!(validate_pandoc_output(&zip(&[("word/document.xml", b"x")]), "docx").is_err());
        assert!(validate_pandoc_output(&zip(&[("[Content_Types].xml", b"x"), ("word/document.xml", b"x")]), "docx").is_ok());
        assert!(validate_pandoc_output(&zip(&[("mimetype", b"incorrect"), ("content.xml", b"x")]), "odt").is_err());
        assert!(validate_pandoc_output(&zip(&[("mimetype", b"application/vnd.oasis.opendocument.text"), ("content.xml", b"x")]), "odt").is_ok());
        assert!(validate_pandoc_output(b"plain text", "rtf").is_err());
        assert!(validate_pandoc_output(br"{\rtf1\ansi test}", "rtf").is_ok());
        assert!(validate_pandoc_output(&zip(&[("mimetype", b"application/epub+zip")]), "epub").is_err());
        assert!(validate_pandoc_output(&zip(&[("mimetype", b"application/epub+zip"), ("META-INF/container.xml", b"x")]), "epub").is_ok());
    }
}
