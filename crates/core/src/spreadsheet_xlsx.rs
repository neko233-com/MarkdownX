//! Apply pending edits to an XLSX archive in memory. This module never writes
//! the original file; callers must use a separate conflict-safe commit.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};
use anyhow::{Context, Result, bail, ensure};
use quick_xml::{Reader, Writer, events::{BytesStart, Event}};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
use crate::spreadsheet_edit::{Address, CellInput};
pub use crate::spreadsheet_styles::NumberFormat;

const MAX_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

pub struct FormatPatch {
    pub snapshot: Vec<u8>,
    /// Original and new cell XF indices, for an undoable caller.
    pub changes: BTreeMap<Address, (u32, u32)>,
}

/// Identify protected formulas that must not be exposed by editing controls.
/// Invalid or ambiguous metadata hides the affected formulas conservatively;
/// worksheet protection is a UI feature, not encryption or access control.
pub fn hidden_formulas(source: &[u8], workbook: &crate::spreadsheet::Workbook) -> BTreeSet<Address> {
    let all_formulas = || workbook.sheets.iter().enumerate().flat_map(|(sheet, data)| data.cells.iter().filter(|cell| cell.formula.is_some()).map(move |cell| Address { sheet, row: cell.row, column: cell.column })).collect::<BTreeSet<_>>();
    let Ok(mut archive) = ZipArchive::new(Cursor::new(source)) else { return all_formulas() };
    let Ok(workbook_xml) = read_entry(&mut archive, "xl/workbook.xml") else { return all_formulas() };
    let Ok(rels) = read_entry(&mut archive, "xl/_rels/workbook.xml.rels") else { return all_formulas() };
    let (Ok(sheets), Ok(targets)) = (sheet_ids(&workbook_xml), relationships(&rels)) else { return all_formulas() };
    let mut hidden = BTreeSet::new();
    let mut styles: Option<Option<Vec<bool>>> = None;
    for (sheet_index, sheet) in workbook.sheets.iter().enumerate() {
        let positions = sheet.cells.iter().filter(|cell| cell.formula.is_some()).map(|cell| (cell.row, cell.column)).collect::<BTreeSet<_>>();
        if positions.is_empty() { continue; }
        let resolved = (|| -> Result<Vec<(u32, u32)>> {
            let id = sheets.get(sheet_index).context("worksheet index does not exist")?;
            let path = targets.get(id).context("worksheet relationship not found")?;
            ensure!(path.starts_with("xl/worksheets/") && path.ends_with(".xml"), "unsupported worksheet target");
            let xml = read_entry(&mut archive, path)?;
            if !crate::spreadsheet_protection::sheet_is_protected(&xml)? { return Ok(Vec::new()); }
            let flags = styles.get_or_insert_with(|| {
                let path = stylesheet_target(&rels).ok().flatten()?;
                crate::spreadsheet_protection::hidden_styles(&read_entry(&mut archive, &path).ok()?).ok()
            }).as_ref().ok_or_else(|| anyhow::anyhow!("formula protection styles unavailable"))?;
            let effective = crate::spreadsheet_xml::effective_styles(&xml, &positions)?;
            let mut result = Vec::new();
            for (position, (style, alternate)) in effective {
                let primary = flags.get(style as usize).copied().unwrap_or(true);
                let inherited = alternate.is_some_and(|style| flags.get(style as usize).copied().unwrap_or(true));
                if primary || inherited { result.push(position); }
            }
            Ok(result)
        })();
        for (row, column) in resolved.unwrap_or_else(|_| positions.into_iter().collect()) { hidden.insert(Address { sheet: sheet_index, row, column }); }
    }
    hidden
}

/// Prepare a number-format change without modifying the original workbook.
/// The caller is responsible for conflict-safe publication and undo history.
pub fn format_number_range(source: &[u8], a: Address, b: Address, format: NumberFormat) -> Result<FormatPatch> {
    ensure!(a.sheet == b.sheet, "invalid worksheet selection");
    let (top, bottom) = (a.row.min(b.row), a.row.max(b.row));
    let (left, right) = (a.column.min(b.column), a.column.max(b.column));
    ensure!(bottom < 1_048_576 && right < 16_384, "selection outside worksheet limits");
    ensure!(u64::from(bottom - top + 1) * u64::from(right - left + 1) <= 100_000, "selection is too large");
    let mut archive = ZipArchive::new(Cursor::new(source)).context("invalid XLSX archive")?;
    inspect_archive(&mut archive)?;
    let workbook = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let sheets = sheet_ids(&workbook)?;
    let targets = relationships(&rels)?;
    let id = sheets.get(a.sheet).context("worksheet index does not exist")?;
    let path = targets.get(id).context("worksheet relationship not found")?;
    ensure!(path.starts_with("xl/worksheets/") && path.ends_with(".xml"), "unsupported worksheet target");
    let sheet = read_entry(&mut archive, path)?;
    ensure!(crate::spreadsheet_protection::formatting_allowed(&sheet)?, "worksheet formatting is protected");
    let style_path = stylesheet_target(&rels)?.ok_or_else(|| anyhow::anyhow!("workbook has no styles relationship"))?;
    ensure!(style_path != *path && style_path != "xl/workbook.xml" && style_path != "xl/_rels/workbook.xml.rels", "invalid styles relationship target");
    let styles = read_entry(&mut archive, &style_path)?;
    let positions = (top..=bottom).flat_map(|row| (left..=right).map(move |column| (row, column))).collect::<BTreeSet<_>>();
    let effective = crate::spreadsheet_xml::effective_styles(&sheet, &positions)?;
    ensure!(effective.values().all(|(_, alternate)| alternate.is_none()), "cell style inheritance is ambiguous");
    let indices = effective.values().map(|(style, _)| *style).collect::<BTreeSet<_>>();
    let (updated_styles, mapping) = crate::spreadsheet_styles::apply_number_format(&styles, &indices, format)?;
    let mut changes = BTreeMap::new();
    let mut new_styles = BTreeMap::new();
    for ((row, column), (old, _)) in effective {
        let new = mapping[&old];
        if old != new {
            let address = Address { sheet: a.sheet, row, column };
            changes.insert(address, (old, new));
            new_styles.insert((row, column), new);
        }
    }
    if changes.is_empty() { return Ok(FormatPatch { snapshot: source.to_vec(), changes }); }
    let updated_sheet = crate::spreadsheet_xml::patch_style_indices(&sheet, &new_styles)?;
    let replacements = BTreeMap::from([
        (path.clone(), updated_sheet),
        (style_path, updated_styles),
        ("xl/workbook.xml".to_string(), enable_recalculation(&workbook)?),
    ]);
    Ok(FormatPatch { snapshot: write_archive(&mut archive, replacements, None)?, changes })
}

/// Apply previously recorded XF indices, for undo and redo. Values and
/// formulas are left intact; all sheets and indices are checked before output.
pub fn patch_style_history(source: &[u8], edits: &BTreeMap<Address, u32>) -> Result<Vec<u8>> {
    if edits.is_empty() { return Ok(source.to_vec()); }
    ensure!(edits.len() <= 100_000, "too many cells in one edit");
    let mut archive = ZipArchive::new(Cursor::new(source)).context("invalid XLSX archive")?;
    inspect_archive(&mut archive)?;
    let workbook = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let sheets = sheet_ids(&workbook)?;
    let targets = relationships(&rels)?;
    let style_path = stylesheet_target(&rels)?.ok_or_else(|| anyhow::anyhow!("workbook has no styles relationship"))?;
    ensure!(style_path != "xl/workbook.xml" && style_path != "xl/_rels/workbook.xml.rels", "invalid styles relationship target");
    let styles = read_entry(&mut archive, &style_path)?;
    crate::spreadsheet_styles::validate_style_indices(&styles, &edits.values().copied().collect())?;
    let mut per_sheet = BTreeMap::<String, BTreeMap<(u32, u32), u32>>::new();
    for (address, style) in edits {
        ensure!(address.row < 1_048_576 && address.column < 16_384, "cell is outside worksheet limits");
        let id = sheets.get(address.sheet).context("worksheet index does not exist")?;
        let path = targets.get(id).context("worksheet relationship not found")?;
        ensure!(path.starts_with("xl/worksheets/") && path.ends_with(".xml") && *path != style_path, "unsupported worksheet target");
        ensure!(per_sheet.entry(path.clone()).or_default().insert((address.row, address.column), *style).is_none(), "duplicate cell edit");
    }
    let mut replacements = BTreeMap::new();
    for (path, cells) in per_sheet {
        let sheet = read_entry(&mut archive, &path)?;
        ensure!(crate::spreadsheet_protection::formatting_allowed(&sheet)?, "worksheet formatting is protected");
        replacements.insert(path, crate::spreadsheet_xml::patch_style_indices(&sheet, &cells)?);
    }
    replacements.insert("xl/workbook.xml".into(), enable_recalculation(&workbook)?);
    write_archive(&mut archive, replacements, None)
}

/// Produce a new archive without modifying `source`. Formula results are not
/// computed here; Excel is asked to recalculate when the workbook is opened.
pub fn patch(source: &[u8], edits: &BTreeMap<Address, CellInput>) -> Result<Vec<u8>> {
    if edits.is_empty() { return Ok(source.to_vec()); }
    let mut archive = ZipArchive::new(Cursor::new(source)).context("invalid XLSX archive")?;
    inspect_archive(&mut archive)?;
    let workbook = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let sheets = sheet_ids(&workbook)?;
    let targets = relationships(&rels)?;
    let mut per_sheet = BTreeMap::<String, BTreeMap<(u32, u32), CellInput>>::new();
    for (address, value) in edits {
        let id = sheets.get(address.sheet).context("worksheet index does not exist")?;
        let path = targets.get(id).context("worksheet relationship not found")?;
        ensure!(path.starts_with("xl/worksheets/") && path.ends_with(".xml"), "unsupported worksheet target");
        ensure!(per_sheet.entry(path.clone()).or_default().insert((address.row, address.column), value.clone()).is_none(), "duplicate cell edit");
    }
    let mut replacements = BTreeMap::new();
    let mut protection_styles: Option<Vec<bool>> = None;
    for (path, cells) in per_sheet {
        let xml = read_entry(&mut archive, &path)?;
        if crate::spreadsheet_protection::sheet_is_protected(&xml)? {
            if protection_styles.is_none() {
                protection_styles = Some(match stylesheet_target(&rels)? {
                    Some(path) => crate::spreadsheet_protection::locked_styles(&read_entry(&mut archive, &path)?)?,
                    None => vec![true],
                });
            }
            let positions = cells.keys().copied().collect::<BTreeSet<_>>();
            let styles = crate::spreadsheet_xml::effective_styles(&xml, &positions)?;
            let locks = protection_styles.as_ref().unwrap();
            for (style, alternate) in styles.values() {
                for style in std::iter::once(style).chain(alternate.iter()) {
                    ensure!(!*locks.get(*style as usize).ok_or_else(|| anyhow::anyhow!("invalid cell protection style"))?, "worksheet contains locked cells");
                }
            }
        }
        replacements.insert(path, crate::spreadsheet_xml::patch_existing_cells(&xml, &cells)?);
    }
    replacements.insert("xl/workbook.xml".to_string(), enable_recalculation(&workbook)?);
    write_archive(&mut archive, replacements, None)
}

fn inspect_archive(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<BTreeSet<String>> {
    ensure!(archive.len() <= 20_000, "too many archive entries");
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        ensure!(names.insert(file.name().to_string()), "duplicate archive entry");
        ensure!(!file.name().starts_with("_xmlsignatures/"), "signed workbooks cannot be edited safely");
        ensure!(file.size() <= MAX_ENTRY_BYTES, "archive entry is too large");
        total = total.checked_add(file.size()).context("archive size overflow")?;
        ensure!(total <= MAX_TOTAL_BYTES, "workbook is too large");
    }
    Ok(names)
}

fn write_archive(archive: &mut ZipArchive<Cursor<&[u8]>>, mut replacements: BTreeMap<String, Vec<u8>>, new_entry: Option<(String, Vec<u8>)>) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.set_raw_comment(archive.comment().into())?;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        if let Some(data) = replacements.remove(entry.name()) {
            let options = SimpleFileOptions::default().compression_method(entry.compression());
            writer.start_file(entry.name(), options)?;
            writer.write_all(&data)?;
        } else { writer.raw_copy_file(entry)?; }
    }
    ensure!(replacements.is_empty(), "modified archive entry is missing");
    if let Some((name, data)) = new_entry {
        writer.start_file(name, SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated))?;
        writer.write_all(&data)?;
    }
    Ok(writer.finish()?.into_inner())
}

fn write_archive_without(archive: &mut ZipArchive<Cursor<&[u8]>>, mut replacements: BTreeMap<String, Vec<u8>>, removed: &str) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.set_raw_comment(archive.comment().into())?;
    let mut found = false;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        if entry.name() == removed { ensure!(!found, "duplicate worksheet entry"); found = true; continue; }
        if let Some(data) = replacements.remove(entry.name()) {
            let options = SimpleFileOptions::default().compression_method(entry.compression());
            writer.start_file(entry.name(), options)?;
            writer.write_all(&data)?;
        } else { writer.raw_copy_file(entry)?; }
    }
    ensure!(found && replacements.is_empty(), "modified archive entry is missing");
    Ok(writer.finish()?.into_inner())
}

/// Append a blank worksheet without rewriting existing sheet data or other
/// package parts. The caller publishes the returned bytes after conflict checks.
pub fn add_worksheet(source: &[u8], name: &str) -> Result<Vec<u8>> {
    validate_worksheet_name(name)?;
    let mut archive = ZipArchive::new(Cursor::new(source)).context("invalid XLSX archive")?;
    let names = inspect_archive(&mut archive)?;
    ensure!(names.len() < 20_000, "too many archive entries to add a worksheet");
    let workbook = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let types = read_entry(&mut archive, "[Content_Types].xml")?;
    let (sheet_names, max_id) = worksheet_names_and_max_id(&workbook)?;
    ensure!(!sheet_names.iter().any(|old| old.to_lowercase() == name.to_lowercase()), "worksheet name already exists");
    let sheet_id = max_id.checked_add(1).context("worksheet ID overflow")?.to_string();
    let relation_ids = collect_attributes(&rels, b"Relationship", b"Id")?;
    let declared_parts = collect_attributes(&types, b"Override", b"PartName")?;
    let relation = (1..=20_001).map(|i| format!("rIdMarkdownX{i}")).find(|id| !relation_ids.contains(id)).context("no available relationship ID")?;
    let part = (1..=20_001).map(|i| format!("xl/worksheets/sheet{i}.xml"))
        .find(|path| !names.contains(path) && !declared_parts.contains(&format!("/{path}"))).context("no available worksheet path")?;
    let sheet = empty_element("sheet", &[("xmlns:r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships"), ("name", name), ("sheetId", &sheet_id), ("r:id", &relation)])?;
    let rel = empty_element("Relationship", &[("Id", &relation), ("Type", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet"), ("Target", &part[3..])])?;
    let part_name = format!("/{part}");
    let content_type = empty_element("Override", &[("PartName", &part_name), ("ContentType", "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml")])?;
    let updated_workbook = insert_child(&workbook, &[b"workbook", b"sheets"], &sheet)?;
    let updated_workbook = enable_recalculation(&updated_workbook)?;
    let updated_rels = insert_child(&rels, &[b"Relationships"], &rel)?;
    let updated_types = insert_child(&types, &[b"Types"], &content_type)?;
    let replacements = BTreeMap::from([("xl/workbook.xml".into(), updated_workbook), ("xl/_rels/workbook.xml.rels".into(), updated_rels), ("[Content_Types].xml".into(), updated_types)]);
    let xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#.to_vec();
    write_archive(&mut archive, replacements, Some((part, xml)))
}

fn validate_worksheet_name(name: &str) -> Result<()> {
    ensure!(!name.trim().is_empty() && name.encode_utf16().count() <= 31 && !name.starts_with('\'') && !name.ends_with('\'')
        && !name.chars().any(|c| c.is_control() || "[]:*?/\\".contains(c)), "invalid worksheet name");
    Ok(())
}

/// Rename only when no other package content mentions the old name. This is
/// conservative: complex workbook references are rejected rather than rewritten.
pub fn rename_worksheet(source: &[u8], index: usize, name: &str) -> Result<Vec<u8>> {
    validate_worksheet_name(name)?;
    let mut archive = ZipArchive::new(Cursor::new(source)).context("invalid XLSX archive")?;
    let entries = inspect_archive(&mut archive)?;
    let workbook = read_entry(&mut archive, "xl/workbook.xml")?;
    let (names, _) = worksheet_names_and_max_id(&workbook)?;
    let old = names.get(index).context("worksheet index does not exist")?;
    if old == name { return Ok(source.to_vec()); }
    ensure!(!names.iter().enumerate().any(|(i, other)| i != index && other.to_lowercase() == name.to_lowercase()), "worksheet name already exists");
    ensure!(!old.to_ascii_lowercase().contains("_x"), "worksheet name has unsupported escape-like text");
    let needle = old.to_lowercase();
    for path in entries {
        if path.ends_with('/') { continue; }
        ensure!(!path.to_lowercase().contains(&needle), "worksheet name appears in a package path");
        if path.starts_with("xl/media/") || path.starts_with("docProps/thumbnail.") { continue; }
        ensure!(!path.starts_with("customXml/"), "workbook contains unsupported custom XML");
        ensure!(path.ends_with(".xml") || path.ends_with(".rels") || path.ends_with(".vml"), "workbook contains unsupported binary parts");
        let data = if path == "xl/workbook.xml" { workbook.clone() } else { read_entry(&mut archive, &path)? };
        ensure!(!xml_mentions_sheet(&data, &needle, (path == "xl/workbook.xml").then_some(index))?, "worksheet name is referenced elsewhere; safe rename is unavailable");
    }
    let renamed = rename_sheet_element(&workbook, index, name)?;
    write_archive(&mut archive, BTreeMap::from([("xl/workbook.xml".into(), renamed)]), None)
}

/// Remove a worksheet only when references and dependent package parts can be
/// ruled out. The caller confirms deletion and publishes after conflict checks.
pub fn delete_worksheet(source: &[u8], index: usize) -> Result<Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(source)).context("invalid XLSX archive")?;
    let entries = inspect_archive(&mut archive)?;
    ensure!(!entries.contains("xl/calcChain.xml"), "workbook calculation chain is not supported for sheet deletion");
    let workbook = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let types = read_entry(&mut archive, "[Content_Types].xml")?;
    let (names, _) = worksheet_names_and_max_id(&workbook)?;
    ensure!(names.len() > 1, "cannot delete the last worksheet");
    let old = names.get(index).context("worksheet index does not exist")?;
    ensure!(!old.to_ascii_lowercase().contains("_x"), "worksheet name has unsupported escape-like text");
    let fallback = remaining_visible_sheet(&workbook, index)?.context("cannot delete the last visible worksheet")?;
    let ids = sheet_ids(&workbook)?;
    let relation = ids.get(index).context("worksheet relationship not found")?;
    let targets = relationships(&rels)?;
    let part = targets.get(relation).context("worksheet relationship not found")?;
    ensure!(part.starts_with("xl/worksheets/") && part.ends_with(".xml") && targets.values().filter(|target| *target == part).count() == 1, "unsupported worksheet target");
    let basename = part.rsplit('/').next().context("invalid worksheet path")?;
    let sheet_rels = format!("xl/worksheets/_rels/{basename}.rels");
    ensure!(!entries.contains(&sheet_rels), "worksheet has dependent relationships");
    validate_sheet_has_no_dependencies(&read_entry(&mut archive, part)?)?;
    validate_workbook_delete_metadata(&workbook)?;
    let old_lower = old.to_lowercase();
    let part_lower = basename.to_lowercase();
    let part_name = format!("/{part}");
    for path in &entries {
        if path.ends_with('/') || path == part { continue; }
        ensure!(!path.to_lowercase().contains(&old_lower) && !path.to_lowercase().contains(&part_lower), "worksheet name or part appears in a package path");
        if path.starts_with("xl/media/") || path.starts_with("docProps/thumbnail.") { continue; }
        ensure!(!path.starts_with("customXml/") && path != "xl/metadata.xml", "workbook contains unsupported sheet-indexed metadata");
        ensure!(path.ends_with(".xml") || path.ends_with(".rels") || path.ends_with(".vml"), "workbook contains unsupported binary parts");
        let data = if path == "xl/workbook.xml" { workbook.clone() } else { read_entry(&mut archive, path)? };
        let ignored = if path == "xl/_rels/workbook.xml.rels" { Some((b"Relationship".as_slice(), b"Id".as_slice(), relation.as_str())) }
            else if path == "[Content_Types].xml" { Some((b"Override".as_slice(), b"PartName".as_slice(), part_name.as_str())) } else { None };
        let ignored_sheet = (path == "xl/workbook.xml").then_some(index);
        ensure!(!xml_mentions_except(&data, &old_lower, ignored_sheet, true, ignored)? && !xml_mentions_except(&data, &part_lower, ignored_sheet, true, ignored)?, "worksheet is referenced elsewhere; safe deletion is unavailable");
    }
    let workbook = remove_empty_element(&workbook, &[b"workbook", b"sheets"], b"sheet", b"r:id", relation)?;
    let workbook = adjust_workbook_views(&workbook, index, names.len(), fallback)?;
    let workbook = enable_recalculation(&workbook)?;
    let rels = remove_empty_element(&rels, &[b"Relationships"], b"Relationship", b"Id", relation)?;
    let types = remove_empty_element(&types, &[b"Types"], b"Override", b"PartName", &part_name)?;
    write_archive_without(&mut archive, BTreeMap::from([("xl/workbook.xml".into(), workbook), ("xl/_rels/workbook.xml.rels".into(), rels), ("[Content_Types].xml".into(), types)]), part)
}

fn mentions_sheet(value: &str, needle: &str) -> Result<bool> {
    let has_xstring_escape = value.as_bytes().windows(7).any(|s| s[0] == b'_' && (s[1] == b'x' || s[1] == b'X') && s[2..6].iter().all(u8::is_ascii_hexdigit) && s[6] == b'_');
    ensure!(!has_xstring_escape, "workbook contains unsupported escaped text");
    Ok(value.replace("''", "'").to_lowercase().contains(needle))
}

fn xml_mentions_sheet(xml: &[u8], needle: &str, ignore_sheet: Option<usize>) -> Result<bool> {
    xml_mentions_except(xml, needle, ignore_sheet, false, None)
}

fn xml_mentions_except(xml: &[u8], needle: &str, ignore_sheet: Option<usize>, remove_sheet: bool, ignored_element: Option<(&[u8], &[u8], &str)>) -> Result<bool> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut sheet_index = 0usize;
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let is_sheet = e.name().as_ref() == b"sheet" && stack.as_slice() == [b"workbook".to_vec(), b"sheets".to_vec()];
                let ignored = is_sheet && ignore_sheet == Some(sheet_index);
                let ignored_part = if let Some((element, key, value)) = ignored_element {
                    e.name().as_ref() == element && xml_attribute(e, key)?.as_deref() == Some(value)
                } else { false };
                if is_sheet { sheet_index += 1; }
                if e.local_name().as_ref() == b"extLst" { bail!("workbook contains unsupported extensions"); }
                if remove_sheet && ignore_sheet.is_none() && [b"sheetId".as_slice(), b"sheetIndex", b"localSheetId", b"tabId"].contains(&e.local_name().as_ref()) {
                    bail!("workbook has unsupported sheet-indexed metadata");
                }
                for attr in e.attributes() {
                    let attr = attr?;
                    if ignored_part || (ignored && (remove_sheet || attr.key.as_ref() == b"name")) { continue; }
                    if remove_sheet && ignore_sheet.is_none() && [b"sheetId".as_slice(), b"sheetIndex", b"localSheetId", b"tabId", b"activeTab", b"firstSheet"].contains(&attr.key.as_ref()) {
                        bail!("workbook has unsupported sheet-indexed metadata");
                    }
                    if mentions_sheet(&attr.unescape_value()?, needle)? { return Ok(true); }
                }
                if matches!(event, Event::Start(_)) { stack.push(e.name().as_ref().to_vec()); }
            }
            Event::Text(text) => { if mentions_sheet(&text.unescape()?, needle)? { return Ok(true); } }
            Event::CData(_) | Event::Comment(_) | Event::PI(_) => bail!("workbook contains unsupported split XML content"),
            Event::End(_) => { ensure!(stack.pop().is_some(), "invalid package XML"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty(), "incomplete package XML");
    Ok(false)
}

fn rename_sheet_element(xml: &[u8], index: usize, name: &str) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut sheet_index = 0usize;
    let mut replacement = None;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event()?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                if e.name().as_ref() == b"sheet" && stack.as_slice() == [b"workbook".to_vec(), b"sheets".to_vec()] {
                    if sheet_index == index {
                        let mut element = BytesStart::new("sheet");
                        let mut found = false;
                        for attr in e.attributes() {
                            let attr = attr?;
                            if attr.key.as_ref() == b"name" { element.push_attribute(("name", name)); found = true; }
                            else { element.push_attribute(attr); }
                        }
                        ensure!(found, "worksheet has no name");
                        let mut writer = Writer::new(Vec::new());
                        if matches!(event, Event::Empty(_)) { writer.write_event(Event::Empty(element))?; }
                        else { writer.write_event(Event::Start(element))?; }
                        replacement = Some((start, reader.buffer_position() as usize, writer.into_inner()));
                    }
                    sheet_index += 1;
                }
                if matches!(event, Event::Start(_)) { stack.push(e.name().as_ref().to_vec()); }
            }
            Event::End(_) => { ensure!(stack.pop().is_some(), "invalid workbook XML"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty(), "incomplete workbook XML");
    let (start, end, element) = replacement.context("worksheet index does not exist")?;
    let mut output = Vec::with_capacity(xml.len() + element.len());
    output.extend_from_slice(&xml[..start]);
    output.extend_from_slice(&element);
    output.extend_from_slice(&xml[end..]);
    Ok(output)
}

fn xml_attribute(e: &BytesStart<'_>, key: &[u8]) -> Result<Option<String>> {
    for attr in e.attributes() { let attr = attr?; if attr.key.as_ref() == key { return Ok(Some(attr.unescape_value()?.into_owned())); } }
    Ok(None)
}

fn remaining_visible_sheet(xml: &[u8], removed: usize) -> Result<Option<usize>> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut index = 0usize;
    let mut visible = None;
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                if e.name().as_ref() == b"sheet" && stack.as_slice() == [b"workbook".to_vec(), b"sheets".to_vec()] {
                    let state = xml_attribute(e, b"state")?.unwrap_or_else(|| "visible".into());
                    ensure!(["visible", "hidden", "veryHidden"].contains(&state.as_str()), "unsupported worksheet visibility");
                    if index != removed && state == "visible" && visible.is_none() { visible = Some(if index > removed { index - 1 } else { index }); }
                    index += 1;
                }
                if matches!(event, Event::Start(_)) { stack.push(e.name().as_ref().to_vec()); }
            }
            Event::End(_) => { ensure!(stack.pop().is_some(), "invalid workbook XML"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty() && removed < index, "worksheet index does not exist");
    Ok(visible)
}

fn validate_sheet_has_no_dependencies(xml: &[u8]) -> Result<()> {
    let mut reader = Reader::from_reader(xml);
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) => {
                ensure!(![b"drawing".as_slice(), b"legacyDrawing", b"legacyDrawingHF", b"tablePart", b"hyperlink", b"oleObject", b"picture", b"extLst"].contains(&e.local_name().as_ref()), "worksheet has dependent content");
                for attr in e.attributes() { ensure!(!attr?.key.as_ref().starts_with(b"r:"), "worksheet has dependent relationships"); }
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

fn validate_workbook_delete_metadata(xml: &[u8]) -> Result<()> {
    let mut reader = Reader::from_reader(xml);
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) => {
                ensure!(![b"customWorkbookViews".as_slice(), b"pivotCaches", b"externalReferences"].contains(&e.local_name().as_ref()), "workbook has unsupported sheet-indexed metadata");
                for attr in e.attributes() { ensure!(attr?.key.as_ref() != b"localSheetId", "workbook has unsupported sheet-indexed metadata"); }
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

fn remove_empty_element(xml: &[u8], parent: &[&[u8]], element: &[u8], key: &[u8], value: &str) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut removed = None;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event()?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                if e.name().as_ref() == element && stack.len() == parent.len() && stack.iter().zip(parent).all(|(a, b)| a.as_slice() == *b) && xml_attribute(e, key)?.as_deref() == Some(value) {
                    ensure!(matches!(event, Event::Empty(_)) && removed.replace((start, reader.buffer_position() as usize)).is_none(), "unsupported or duplicate package element");
                }
                if matches!(event, Event::Start(_)) { stack.push(e.name().as_ref().to_vec()); }
            }
            Event::End(_) => { ensure!(stack.pop().is_some(), "invalid package XML"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty(), "incomplete package XML");
    let (start, end) = removed.context("package element not found")?;
    let mut output = Vec::with_capacity(xml.len());
    output.extend_from_slice(&xml[..start]);
    output.extend_from_slice(&xml[end..]);
    Ok(output)
}

fn adjust_workbook_views(xml: &[u8], removed: usize, old_count: usize, fallback: usize) -> Result<Vec<u8>> {
    ensure!(old_count > 1 && removed < old_count && fallback < old_count - 1, "invalid workbook view index");
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut output = Vec::new();
    let mut written = 0usize;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event()?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                if e.name().as_ref() == b"workbookView" && stack.as_slice() == [b"workbook".to_vec(), b"bookViews".to_vec()] {
                    let mut updated = BytesStart::new("workbookView");
                    for attr in e.attributes() {
                        let attr = attr?;
                        if [b"activeTab".as_slice(), b"firstSheet"].contains(&attr.key.as_ref()) {
                            let old = attr.unescape_value()?.parse::<usize>()?;
                            ensure!(old < old_count, "invalid workbook view index");
                            let new = if old > removed { old - 1 } else if old == removed { fallback } else { old };
                            let key = std::str::from_utf8(attr.key.as_ref())?;
                            let value = new.to_string();
                            updated.push_attribute((key, value.as_str()));
                        } else { updated.push_attribute(attr); }
                    }
                    let mut writer = Writer::new(Vec::new());
                    if matches!(event, Event::Empty(_)) { writer.write_event(Event::Empty(updated))?; }
                    else { writer.write_event(Event::Start(updated))?; }
                    output.extend_from_slice(&xml[written..start]);
                    output.extend_from_slice(&writer.into_inner());
                    written = reader.buffer_position() as usize;
                }
                if matches!(event, Event::Start(_)) { stack.push(e.name().as_ref().to_vec()); }
            }
            Event::End(_) => { ensure!(stack.pop().is_some(), "invalid workbook XML"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty(), "incomplete workbook XML");
    output.extend_from_slice(&xml[written..]);
    Ok(output)
}

fn worksheet_names_and_max_id(xml: &[u8]) -> Result<(Vec<String>, u32)> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut names = Vec::new();
    let mut ids = BTreeSet::new();
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                if e.local_name().as_ref() == b"workbookProtection" && xml_attribute(e, b"lockStructure")?.is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")) { bail!("workbook structure is protected"); }
                if e.name().as_ref() == b"sheet" && stack.as_slice() == [b"workbook".to_vec(), b"sheets".to_vec()] {
                    let name = xml_attribute(e, b"name")?.context("worksheet has no name")?;
                    let id = xml_attribute(e, b"sheetId")?.context("worksheet has no ID")?.parse::<u32>()?;
                    ensure!(id > 0 && ids.insert(id), "duplicate or invalid worksheet ID");
                    names.push(name);
                }
                if matches!(event, Event::Start(_)) { stack.push(e.name().as_ref().to_vec()); }
            }
            Event::End(_) => { ensure!(stack.pop().is_some(), "invalid workbook XML"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty() && !names.is_empty(), "unsupported workbook sheet list");
    Ok((names, ids.into_iter().max().unwrap_or(0)))
}

fn collect_attributes(xml: &[u8], element: &[u8], key: &[u8]) -> Result<BTreeSet<String>> {
    let mut reader = Reader::from_reader(xml);
    let mut values = BTreeSet::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == element => {
                let value = xml_attribute(&e, key)?.context("package metadata is missing an attribute")?;
                ensure!(values.insert(value), "duplicate package metadata");
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(values)
}

fn empty_element(name: &str, attributes: &[(&str, &str)]) -> Result<Vec<u8>> {
    let mut element = BytesStart::new(name);
    for &(key, value) in attributes { element.push_attribute((key, value)); }
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Empty(element))?;
    Ok(writer.into_inner())
}

fn insert_child(xml: &[u8], path: &[&[u8]], child: &[u8]) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut position = None;
    loop {
        let start = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Start(e) => stack.push(e.name().as_ref().to_vec()),
            Event::End(_) => {
                if stack.len() == path.len() && stack.iter().zip(path).all(|(a, b)| a.as_slice() == *b) {
                    ensure!(position.replace(start).is_none(), "duplicate package parent element");
                }
                ensure!(stack.pop().is_some(), "invalid package XML");
            }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty(), "incomplete package XML");
    let at = position.context("package parent element not found")?;
    let mut output = Vec::with_capacity(xml.len() + child.len());
    output.extend_from_slice(&xml[..at]);
    output.extend_from_slice(child);
    output.extend_from_slice(&xml[at..]);
    Ok(output)
}

fn read_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>> {
    let mut file = archive.by_name(name).with_context(|| format!("missing {name}"))?;
    ensure!(file.size() <= MAX_ENTRY_BYTES, "archive entry is too large");
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    Ok(data)
}

fn sheet_ids(xml: &[u8]) -> Result<Vec<String>> {
    let mut reader = Reader::from_reader(xml);
    let mut ids = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"sheet" => {
                let mut id = None;
                for attr in e.attributes() { let attr = attr?; if attr.key.as_ref() == b"r:id" { id = Some(attr.unescape_value()?.into_owned()); } }
                ids.push(id.context("worksheet has no relationship ID")?);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(ids)
}

fn relationships(xml: &[u8]) -> Result<BTreeMap<String, String>> {
    let mut reader = Reader::from_reader(xml);
    let mut paths = BTreeMap::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                let mut id = None;
                let mut target = None;
                let mut external = false;
                for attr in e.attributes() {
                    let attr = attr?;
                    match attr.key.as_ref() {
                        b"Id" => id = Some(attr.unescape_value()?.into_owned()),
                        b"Target" => target = Some(attr.unescape_value()?.into_owned()),
                        b"TargetMode" => external = attr.unescape_value()?.eq_ignore_ascii_case("external"),
                        _ => {}
                    }
                }
                if let (Some(id), Some(target)) = (id, target) && !external {
                    let path = if let Some(path) = target.strip_prefix('/') { path.to_string() } else { format!("xl/{target}") };
                    if !path.contains('\\') && !path.split('/').any(|part| part == ".." || part == ".") {
                        ensure!(paths.insert(id, path).is_none(), "duplicate relationship ID");
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(paths)
}

fn stylesheet_target(xml: &[u8]) -> Result<Option<String>> {
    let mut reader = Reader::from_reader(xml);
    let mut result = None;
    let mut stack = Vec::<Vec<u8>>::new();
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.local_name().as_ref().to_vec();
                if name == b"Relationship" && stack.len() == 1 && stack[0] == b"Relationships" {
                    let kind = xml_attribute(element, b"Type")?;
                    if matches!(kind.as_deref(), Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" | "http://purl.oclc.org/ooxml/officeDocument/relationships/styles")) {
                        ensure!(result.is_none(), "duplicate styles relationship");
                        ensure!(!xml_attribute(element, b"TargetMode")?.is_some_and(|value| value.eq_ignore_ascii_case("external")), "external styles are unsupported");
                        let target = xml_attribute(element, b"Target")?.ok_or_else(|| anyhow::anyhow!("styles relationship has no target"))?;
                        let path = if let Some(path) = target.strip_prefix('/') { path.to_string() } else { format!("xl/{target}") };
                        ensure!(!path.contains('\\') && !path.split('/').any(|part| part == ".." || part == "." || part.is_empty()), "invalid styles relationship");
                        result = Some(path);
                    }
                }
                if matches!(event, Event::Start(_)) { stack.push(name); }
            }
            Event::End(element) => { ensure!(stack.pop().as_deref() == Some(element.local_name().as_ref()), "invalid relationships XML"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty(), "incomplete relationships XML");
    Ok(result)
}

fn enable_recalculation(xml: &[u8]) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(xml);
    let mut changed = false;
    let mut output = Vec::new();
    let mut written = 0usize;
    loop {
        let start = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Empty(e) if e.local_name().as_ref() == b"calcPr" => {
                ensure!(!changed && e.name().as_ref() == b"calcPr", "unsupported calculation settings");
                let mut replacement = BytesStart::new("calcPr");
                for attr in e.attributes() {
                    let attr = attr?;
                    if ![b"calcMode".as_slice(), b"fullCalcOnLoad", b"forceFullCalc"].contains(&attr.key.as_ref()) { replacement.push_attribute(attr); }
                }
                replacement.push_attribute(("calcMode", "auto"));
                replacement.push_attribute(("fullCalcOnLoad", "1"));
                replacement.push_attribute(("forceFullCalc", "1"));
                let mut writer = Writer::new(Vec::new());
                writer.write_event(Event::Empty(replacement))?;
                output.extend_from_slice(&xml[written..start]);
                output.extend_from_slice(&writer.into_inner());
                written = reader.buffer_position() as usize;
                changed = true;
            }
            Event::Start(e) if e.local_name().as_ref() == b"calcPr" => bail!("unsupported calculation settings"),
            Event::End(e) if e.local_name().as_ref() == b"workbook" && !changed => {
                ensure!(e.name().as_ref() == b"workbook", "prefixed workbook is not supported yet");
                output.extend_from_slice(&xml[written..start]);
                output.extend_from_slice(br#"<calcPr calcMode="auto" fullCalcOnLoad="1" forceFullCalc="1"/>"#);
                written = start;
                changed = true;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(changed, "workbook calculation settings could not be updated");
    output.extend_from_slice(&xml[written..]);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{Reader as _, Xlsx};

    #[test]
    fn hides_only_protected_formulas_and_fails_closed_for_bad_styles() {
        let source = include_bytes!("../tests/fixtures/hidden_formulas.xlsx");
        let workbook = crate::spreadsheet::read_xlsx_snapshot(source).unwrap();
        let a = |sheet, column| Address { sheet, row: 0, column };
        assert_eq!(hidden_formulas(source, &workbook), BTreeSet::from([a(0, 1), a(0, 3)]));
        let mut archive = ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let malformed = write_archive(&mut archive, BTreeMap::from([("xl/styles.xml".into(), b"<invalid/>".to_vec())]), None).unwrap();
        assert_eq!(hidden_formulas(&malformed, &workbook), BTreeSet::from([a(0, 1), a(0, 2), a(0, 3)]));
    }

    #[test]
    fn number_formats_preserve_values_formulas_and_other_parts() {
        let source = include_bytes!("../tests/fixtures/number_formats.xlsx");
        let a = Address { sheet: 0, row: 0, column: 0 };
        let b = Address { sheet: 0, row: 1, column: 1 };
        let original = crate::spreadsheet::read_xlsx_snapshot(source).unwrap();
        let result = format_number_range(source, a, b, NumberFormat::Decimal2).unwrap();
        assert!(!result.changes.is_empty());
        let updated = crate::spreadsheet::read_xlsx_snapshot(&result.snapshot).unwrap();
        for cell in &original.sheets[0].cells {
            let actual = updated.sheets[0].cells.iter().find(|item| item.row == cell.row && item.column == cell.column).unwrap();
            assert_eq!(actual.value, cell.value);
            assert_eq!(actual.formula, cell.formula);
        }
        let mut before = ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let mut after = ZipArchive::new(Cursor::new(result.snapshot.as_slice())).unwrap();
        for index in 0..before.len() {
            let name = before.by_index(index).unwrap().name().to_string();
            if ["xl/workbook.xml", "xl/styles.xml", "xl/worksheets/sheet1.xml"].contains(&name.as_str()) { continue; }
            assert_eq!(read_entry(&mut before, &name).unwrap(), read_entry(&mut after, &name).unwrap(), "{name}");
        }
        let repeated = format_number_range(&result.snapshot, a, b, NumberFormat::Decimal2).unwrap();
        assert!(repeated.changes.is_empty());
        assert_eq!(repeated.snapshot, result.snapshot);
        assert!(format_number_range(source, a, Address { sheet: 1, ..b }, NumberFormat::General).is_err());
        assert!(format_number_range(source, a, Address { row: 1_048_576, ..b }, NumberFormat::General).is_err());
        let protected = include_bytes!("../tests/fixtures/sheet_protection.xlsx");
        assert!(format_number_range(protected, a, b, NumberFormat::Percent2).is_err());
        let mut archive = ZipArchive::new(Cursor::new(protected.as_slice())).unwrap();
        let sheet = String::from_utf8(read_entry(&mut archive, "xl/worksheets/sheet1.xml").unwrap()).unwrap();
        let allowed = sheet.replacen("formatCells=\"1\"", "formatCells=\"0\"", 1);
        assert_ne!(sheet, allowed);
        let allowed = write_archive(&mut archive, BTreeMap::from([("xl/worksheets/sheet1.xml".into(), allowed.into_bytes())]), None).unwrap();
        let formatted = format_number_range(&allowed, a, a, NumberFormat::Percent2).unwrap();
        let reverse = formatted.changes.iter().map(|(&address, &(before, _))| (address, before)).collect();
        let restored = patch_style_history(&formatted.snapshot, &reverse).unwrap();
        assert!(patch(&restored, &BTreeMap::from([(a, CellInput::Text("locked".into()))])).is_err());
        assert!(patch_style_history(&restored, &BTreeMap::from([(a, u32::MAX)])).is_err());
    }

    #[test]
    fn protected_sheet_respects_locked_and_unlocked_cells() {
        let source = include_bytes!("../tests/fixtures/sheet_protection.xlsx");
        let at = |sheet, row, column| Address { sheet, row, column };
        let edit = |address| BTreeMap::from([(address, CellInput::Text("changed".into()))]);
        assert!(patch(source, &edit(at(0, 0, 0))).is_err());
        assert!(patch(source, &edit(at(0, 1, 1))).is_err());
        for address in [at(0, 0, 1), at(0, 0, 2), at(0, 0, 3), at(0, 1, 0), at(1, 0, 0)] {
            let updated = patch(source, &edit(address)).unwrap();
            let book = crate::spreadsheet::read_xlsx_snapshot(&updated).unwrap();
            assert!(book.sheets[address.sheet].cells.iter().any(|cell| cell.row == address.row && cell.column == address.column && cell.value == "changed"));
        }
        let mixed = BTreeMap::from([(at(0, 0, 0), CellInput::Text("no".into())), (at(0, 0, 1), CellInput::Text("yes".into()))]);
        assert!(patch(source, &mixed).is_err());

        let mut archive = ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let sheet = String::from_utf8(read_entry(&mut archive, "xl/worksheets/sheet1.xml").unwrap()).unwrap();
        let unprotected = sheet.replacen("sheet=\"1\"", "sheet=\"0\"", 1);
        assert_ne!(sheet, unprotected);
        let bytes = write_archive(&mut archive, BTreeMap::from([("xl/worksheets/sheet1.xml".into(), unprotected.into_bytes())]), None).unwrap();
        assert!(patch(&bytes, &edit(at(0, 0, 0))).is_ok());

        let mut archive = ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let rels = String::from_utf8(read_entry(&mut archive, "xl/_rels/workbook.xml.rels").unwrap()).unwrap();
        let relocated = rels.replacen("Target=\"styles.xml\"", "Target=\"alt/styles.xml\"", 1);
        assert_ne!(rels, relocated);
        let styles = read_entry(&mut archive, "xl/styles.xml").unwrap();
        let bytes = write_archive(&mut archive, BTreeMap::from([("xl/_rels/workbook.xml.rels".into(), relocated.into_bytes())]), Some(("xl/alt/styles.xml".into(), styles))).unwrap();
        assert!(patch(&bytes, &edit(at(0, 0, 0))).is_err());
        assert!(patch(&bytes, &edit(at(0, 0, 1))).is_ok());

        let ambiguous = include_bytes!("../tests/fixtures/sheet_protection_ambiguous.xlsx");
        assert!(patch(ambiguous, &edit(at(0, 0, 0))).is_err());
        assert!(patch(ambiguous, &edit(at(0, 1, 0))).is_ok());

        let mut archive = ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let rels = String::from_utf8(read_entry(&mut archive, "xl/_rels/workbook.xml.rels").unwrap()).unwrap();
        let removed = rels.replace("<Relationship Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\" Id=\"rId3\"/>", "");
        assert_ne!(rels, removed);
        let bytes = write_archive(&mut archive, BTreeMap::from([("xl/_rels/workbook.xml.rels".into(), removed.into_bytes())]), None).unwrap();
        assert!(patch(&bytes, &edit(at(0, 0, 0))).is_err());
        assert!(patch(&bytes, &edit(at(0, 0, 1))).is_err());
    }

    #[test]
    fn appends_a_sheet_and_preserves_existing_workbook_parts() {
        let source = include_bytes!("../tests/fixtures/spreadsheet.xlsx");
        let added = add_worksheet(source, "策划 & <测试>").unwrap();
        let loaded = crate::spreadsheet::read_xlsx_snapshot(&added).unwrap();
        assert_eq!(loaded.sheets.len(), 3);
        assert_eq!(loaded.sheets[2].name, "策划 & <测试>");
        assert!(loaded.sheets[2].cells.is_empty());
        let edit = BTreeMap::from([(Address { sheet: 2, row: 0, column: 0 }, CellInput::Number(123.0))]);
        let edited = patch(&added, &edit).unwrap();
        assert!(crate::spreadsheet::read_xlsx_snapshot(&edited).unwrap().sheets[2].cells.iter().any(|c| c.value == 123.0));
        let mut before = ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let mut after = ZipArchive::new(Cursor::new(added.as_slice())).unwrap();
        assert_eq!(after.len(), before.len() + 1);
        for i in 0..before.len() {
            let name = before.by_index(i).unwrap().name().to_string();
            if ["xl/workbook.xml", "xl/_rels/workbook.xml.rels", "[Content_Types].xml"].contains(&name.as_str()) { continue; }
            assert_eq!(read_entry(&mut before, &name).unwrap(), read_entry(&mut after, &name).unwrap(), "{name}");
        }
        for invalid in ["", "  ", "x/y", "bad:name", "'bad", "a very long worksheet name over 31 chars", "数据", "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀"] {
            assert!(add_worksheet(source, invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn refuses_structure_protected_and_signed_workbooks() {
        let source = include_bytes!("../tests/fixtures/spreadsheet.xlsx");
        let mut archive = ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let workbook = read_entry(&mut archive, "xl/workbook.xml").unwrap();
        let protected = insert_child(&workbook, &[b"workbook"], br#"<workbookProtection lockStructure="1"/>"#).unwrap();
        let protected = write_archive(&mut archive, BTreeMap::from([("xl/workbook.xml".into(), protected)]), None).unwrap();
        assert!(add_worksheet(&protected, "新工作表").is_err());
        let mut archive = ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let signed = write_archive(&mut archive, BTreeMap::new(), Some(("_xmlsignatures/sig1.xml".into(), b"signature".to_vec()))).unwrap();
        assert!(add_worksheet(&signed, "新工作表").is_err());
    }

    #[test]
    fn renames_only_unreferenced_sheets_without_rewriting_other_parts() {
        let source = include_bytes!("../../../assets/empty-workbook.xlsx");
        let added = add_worksheet(source, "数值").unwrap();
        let edited = patch(&added, &BTreeMap::from([(Address { sheet: 1, row: 0, column: 0 }, CellInput::Number(42.0))])).unwrap();
        let renamed = rename_worksheet(&edited, 1, "平衡 & 角色").unwrap();
        let book = crate::spreadsheet::read_xlsx_snapshot(&renamed).unwrap();
        assert_eq!(book.sheets[1].name, "平衡 & 角色");
        assert!(book.sheets[1].cells.iter().any(|c| c.value == 42.0));
        let mut before = ZipArchive::new(Cursor::new(edited.as_slice())).unwrap();
        let mut after = ZipArchive::new(Cursor::new(renamed.as_slice())).unwrap();
        for i in 0..before.len() {
            let part = before.by_index(i).unwrap().name().to_string();
            if part == "xl/workbook.xml" { continue; }
            assert_eq!(read_entry(&mut before, &part).unwrap(), read_entry(&mut after, &part).unwrap(), "{part}");
        }
        assert!(rename_worksheet(&edited, 1, "sheet1").is_err());
        assert!(rename_worksheet(&edited, 1, "bad/name").is_err());
        assert_eq!(rename_worksheet(&edited, 1, "数值").unwrap(), edited);
        assert!(rename_worksheet(include_bytes!("../tests/fixtures/calculation.xlsx"), 0, "改名").is_err());
        let mut archive = ZipArchive::new(Cursor::new(edited.as_slice())).unwrap();
        let with_binary = write_archive(&mut archive, BTreeMap::new(), Some(("xl/custom.bin".into(), b"opaque".to_vec()))).unwrap();
        assert!(rename_worksheet(&with_binary, 1, "改名").is_err());
        let mut archive = ZipArchive::new(Cursor::new(edited.as_slice())).unwrap();
        let with_escaped_text = write_archive(&mut archive, BTreeMap::new(), Some(("xl/custom.xml".into(), b"<x>_x6570__x503C_</x>".to_vec()))).unwrap();
        assert!(rename_worksheet(&with_escaped_text, 1, "改名").is_err());
        let mut archive = ZipArchive::new(Cursor::new(edited.as_slice())).unwrap();
        let custom = write_archive(&mut archive, BTreeMap::new(), Some(("customXml/item1.xml".into(), b"<x/>".to_vec()))).unwrap();
        assert!(rename_worksheet(&custom, 1, "改名").is_err());
    }

    #[test]
    fn deletes_unreferenced_sheet_and_preserves_other_sheets() {
        let source = include_bytes!("../../../assets/empty-workbook.xlsx");
        let added = add_worksheet(source, "数值").unwrap();
        let added = add_worksheet(&added, "配置").unwrap();
        let edited = patch(&added, &BTreeMap::from([
            (Address { sheet: 1, row: 0, column: 0 }, CellInput::Number(12.0)),
            (Address { sheet: 2, row: 0, column: 0 }, CellInput::Number(99.0)),
        ])).unwrap();
        let deleted = delete_worksheet(&edited, 1).unwrap();
        let book = crate::spreadsheet::read_xlsx_snapshot(&deleted).unwrap();
        assert_eq!(book.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Sheet1", "配置"]);
        assert!(book.sheets[1].cells.iter().any(|cell| cell.value == 99.0));
        let single = delete_worksheet(&deleted, 0).unwrap();
        assert!(delete_worksheet(&single, 0).is_err());
        assert!(delete_worksheet(include_bytes!("../tests/fixtures/calculation.xlsx"), 0).is_err());
        let mut archive = ZipArchive::new(Cursor::new(edited.as_slice())).unwrap();
        let dependent = write_archive(&mut archive, BTreeMap::new(), Some(("xl/worksheets/_rels/sheet2.xml.rels".into(), b"<Relationships/>".to_vec()))).unwrap();
        assert!(delete_worksheet(&dependent, 1).is_err());
        let mut archive = ZipArchive::new(Cursor::new(edited.as_slice())).unwrap();
        let indexed = write_archive(&mut archive, BTreeMap::new(), Some(("xl/custom.xml".into(), b"<x sheetIndex=\"1\"/>".to_vec()))).unwrap();
        assert!(delete_worksheet(&indexed, 1).is_err());
        let mut archive = ZipArchive::new(Cursor::new(edited.as_slice())).unwrap();
        let custom = write_archive(&mut archive, BTreeMap::new(), Some(("customXml/item1.xml".into(), b"<x/>".to_vec()))).unwrap();
        assert!(delete_worksheet(&custom, 1).is_err());
        let view = br#"<workbook><bookViews><workbookView activeTab="2" firstSheet="1"/></bookViews></workbook>"#;
        let view = String::from_utf8(adjust_workbook_views(view, 1, 3, 0).unwrap()).unwrap();
        assert!(view.contains("activeTab=\"1\"") && view.contains("firstSheet=\"0\""));
        let visibility = br#"<workbook><sheets><sheet name="a"/><sheet name="b" state="hidden"/></sheets></workbook>"#;
        assert_eq!(remaining_visible_sheet(visibility, 0).unwrap(), None);
    }

    #[test]
    fn preserves_real_merged_and_array_ranges() {
        let source = include_bytes!("../tests/fixtures/protected_ranges.xlsx");
        let edit = |row, column| BTreeMap::from([(Address { sheet: 0, row, column }, CellInput::Number(9.0))]);
        assert!(patch(source, &edit(1, 1)).is_err());
        assert!(patch(source, &edit(1, 4)).is_err());
        assert!(patch(source, &edit(0, 3)).is_err());
        let output = patch(source, &edit(0, 0)).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(output.as_slice())).unwrap();
        let sheet = String::from_utf8(read_entry(&mut archive, "xl/worksheets/sheet1.xml").unwrap()).unwrap();
        assert!(sheet.contains("ref=\"A1:B2\""));
        assert!(sheet.contains("ref=\"D1:E2\""));
        assert!(Xlsx::new(Cursor::new(output)).is_ok());
    }

    #[test]
    fn modifies_real_workbook_and_preserves_other_entries() {
        let source = include_bytes!("../tests/fixtures/spreadsheet.xlsx");
        let changes = BTreeMap::from([
            (Address { sheet: 0, row: 0, column: 0 }, CellInput::Number(77.0)),
            (Address { sheet: 0, row: 2, column: 3 }, CellInput::Text("攻击力 & HP".into())),
            (Address { sheet: 0, row: 2, column: 4 }, CellInput::Number(99.0)),
            (Address { sheet: 1, row: 1, column: 1 }, CellInput::Number(42.0)),
        ]);
        let output = patch(source, &changes).unwrap();
        let mut workbook = Xlsx::new(Cursor::new(&output)).unwrap();
        let mut first = workbook.worksheet_cells_reader("数据").unwrap();
        let mut found = false;
        let mut inserted = false;
        let mut new_row = false;
        while let Some(cell) = first.next_cell_with_formula().unwrap() {
            if cell.pos == (2, 3) { assert_eq!(calamine::Data::from(cell.value).to_string(), "攻击力 & HP"); found = true; }
            else if cell.pos == (2, 4) { assert_eq!(calamine::Data::from(cell.value).to_string(), "99"); inserted = true; }
            else if cell.pos == (0, 0) { assert_eq!(calamine::Data::from(cell.value).to_string(), "77"); new_row = true; }
        }
        assert!(found && inserted && new_row);
        drop(first);
        let mut second = workbook.worksheet_cells_reader("Second").unwrap();
        assert_eq!(calamine::Data::from(second.next_cell().unwrap().unwrap().get_value().clone()).to_string(), "42");
        let mut before = ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let mut after = ZipArchive::new(Cursor::new(output.as_slice())).unwrap();
        assert_eq!(before.len(), after.len());
        for i in 0..before.len() {
            let name = before.by_index(i).unwrap().name().to_string();
            let original = read_entry(&mut before, &name).unwrap();
            let revised = read_entry(&mut after, &name).unwrap();
            if name == "xl/workbook.xml" { assert!(String::from_utf8(revised).unwrap().contains("fullCalcOnLoad=\"1\"")); }
            else if name != "xl/worksheets/sheet1.xml" && name != "xl/worksheets/sheet2.xml" { assert_eq!(original, revised, "changed unrelated entry {name}"); }
        }
        assert!(patch(source, &BTreeMap::from([(Address { sheet: 7, row: 0, column: 0 }, CellInput::Number(1.0))])).is_err());
        assert_eq!(source.as_slice(), include_bytes!("../tests/fixtures/spreadsheet.xlsx"));
    }

    #[test]
    fn adds_cells_to_an_empty_workbook_sheet() {
        let source = include_bytes!("../tests/fixtures/spreadsheet.xlsx");
        let mut archive = ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            if entry.name() == "xl/worksheets/sheet1.xml" {
                writer.start_file(entry.name(), SimpleFileOptions::default()).unwrap();
                writer.write_all(br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1"/><sheetData/></worksheet>"#).unwrap();
            } else { writer.raw_copy_file(entry).unwrap(); }
        }
        let blank = writer.finish().unwrap().into_inner();
        let changes = BTreeMap::from([(Address { sheet: 0, row: 8, column: 2 }, CellInput::Text("金币".into()))]);
        let patched = patch(&blank, &changes).unwrap();
        let mut workbook = Xlsx::new(Cursor::new(patched)).unwrap();
        let mut sheet = workbook.worksheet_cells_reader("数据").unwrap();
        let cell = sheet.next_cell().unwrap().unwrap();
        assert_eq!(cell.get_position(), (8, 2));
        assert_eq!(calamine::Data::from(cell.get_value().clone()).to_string(), "金币");
    }
}
