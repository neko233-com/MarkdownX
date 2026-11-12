//! Protection metadata used to avoid changing locked worksheet cells.
use anyhow::{Result, bail, ensure};
use quick_xml::{Reader, events::{BytesStart, Event}};

fn attribute(element: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    let mut value = None;
    for attr in element.attributes() {
        let attr = attr?;
        if attr.key.as_ref() == name {
            ensure!(value.is_none(), "duplicate protection attribute");
            value = Some(attr.unescape_value()?.into_owned());
        }
    }
    Ok(value)
}

fn boolean(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => bail!("invalid protection boolean"),
    }
}

#[derive(Clone, Copy)]
struct SheetProtection { sheet: bool, format_cells_locked: bool }

fn sheet_protection(xml: &[u8]) -> Result<Option<SheetProtection>> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut protection = None;
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.local_name().as_ref().to_vec();
                if name == b"sheetProtection" && stack.len() == 1 && stack[0] == b"worksheet" {
                    ensure!(protection.is_none(), "duplicate sheet protection");
                    let sheet = attribute(element, b"sheet")?.map(|value| boolean(&value)).transpose()?.unwrap_or(true);
                    let format_cells_locked = if sheet { attribute(element, b"formatCells")?.map(|value| boolean(&value)).transpose()?.unwrap_or(true) } else { true };
                    protection = Some(SheetProtection { sheet, format_cells_locked });
                }
                if matches!(event, Event::Start(_)) { stack.push(name); }
            }
            Event::End(_) => { ensure!(stack.pop().is_some(), "invalid worksheet XML"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty(), "incomplete worksheet XML");
    Ok(protection)
}

/// Treat a protection element without an explicit sheet flag conservatively.
pub fn sheet_is_protected(xml: &[u8]) -> Result<bool> { Ok(sheet_protection(xml)?.is_some_and(|protection| protection.sheet)) }

pub fn formatting_allowed(xml: &[u8]) -> Result<bool> {
    Ok(sheet_protection(xml)?.is_none_or(|protection| !protection.sheet || !protection.format_cells_locked))
}

#[derive(Default)]
struct Xf { parent: usize, apply: Option<bool>, locked: Option<bool> }

/// Resolve locked flags for cellXfs. Ambiguous or invalid inheritance fails
/// closed; cells with an explicit unlocked style remain editable.
pub fn locked_styles(xml: &[u8]) -> Result<Vec<bool>> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut parents = Vec::<Xf>::new();
    let mut cells = Vec::<Xf>::new();
    let mut active: Option<(bool, Xf)> = None;
    let mut parent_section = false;
    let mut cell_section = false;
    let mut parent_count = None;
    let mut cell_count = None;
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.local_name().as_ref().to_vec();
                let parent = stack.last().map(Vec::as_slice);
                let table = stack.len() == 2 && stack[0] == b"styleSheet" && (parent == Some(b"cellStyleXfs") || parent == Some(b"cellXfs"));
                let direct_xf = stack.len() == 3 && stack[0] == b"styleSheet" && (stack[1] == b"cellStyleXfs" || stack[1] == b"cellXfs") && parent == Some(b"xf");
                if stack.len() == 1 && parent == Some(b"styleSheet") && name == b"cellStyleXfs" {
                    ensure!(!parent_section, "duplicate style protection table");
                    parent_section = true;
                    parent_count = attribute(element, b"count")?.map(|value| value.parse::<usize>()).transpose()?;
                    ensure!(parent_count.is_none_or(|count| count <= 65_536), "too many parent protection styles");
                }
                if stack.len() == 1 && parent == Some(b"styleSheet") && name == b"cellXfs" {
                    ensure!(!cell_section, "duplicate cell protection table");
                    cell_section = true;
                    cell_count = attribute(element, b"count")?.map(|value| value.parse::<usize>()).transpose()?;
                    ensure!(cell_count.is_none_or(|count| count <= 65_536), "too many cell protection styles");
                }
                if name == b"xf" && table {
                    ensure!(active.is_none(), "nested cell style");
                    let is_parent = parent == Some(b"cellStyleXfs");
                    let xf = Xf {
                        parent: attribute(element, b"xfId")?.map(|value| value.parse()).transpose()?.unwrap_or(0),
                        apply: attribute(element, b"applyProtection")?.map(|value| boolean(&value)).transpose()?,
                        locked: None,
                    };
                    if matches!(event, Event::Empty(_)) {
                        if is_parent { parents.push(xf); } else { cells.push(xf); }
                    } else { active = Some((is_parent, xf)); }
                } else if name == b"protection" && direct_xf && let Some((_, xf)) = &mut active {
                    ensure!(xf.locked.is_none(), "duplicate cell protection");
                    xf.locked = Some(attribute(element, b"locked")?.map(|value| boolean(&value)).transpose()?.unwrap_or(true));
                }
                if matches!(event, Event::Start(_)) { stack.push(name); }
            }
            Event::End(element) => {
                let name = element.local_name().as_ref().to_vec();
                ensure!(stack.pop().as_deref() == Some(name.as_slice()), "invalid style XML");
                if name == b"xf" && stack.len() == 2 && stack[0] == b"styleSheet" && (stack[1] == b"cellXfs" || stack[1] == b"cellStyleXfs") && let Some((is_parent, xf)) = active.take() {
                    if is_parent { parents.push(xf); } else { cells.push(xf); }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        ensure!(parents.len() <= 65_536 && cells.len() <= 65_536, "too many protection styles");
    }
    ensure!(stack.is_empty() && active.is_none() && cell_section && !cells.is_empty(), "invalid cell protection styles");
    ensure!(parent_count.is_none_or(|count| count == parents.len()) && cell_count.is_none_or(|count| count == cells.len()), "style protection count mismatch");
    let parent_locks = parents.iter().map(|xf| if xf.apply == Some(false) { true } else { xf.locked.unwrap_or(true) }).collect::<Vec<_>>();
    let mut locks = Vec::with_capacity(cells.len());
    for xf in cells {
        let inherited = if parent_locks.is_empty() && xf.parent == 0 { true } else { *parent_locks.get(xf.parent).ok_or_else(|| anyhow::anyhow!("invalid parent protection style"))? };
        let locked = if xf.apply == Some(true) { xf.locked.unwrap_or(true) } else {
            if let Some(explicit) = xf.locked { ensure!(explicit == inherited, "ambiguous cell protection inheritance"); }
            inherited
        };
        locks.push(locked);
    }
    Ok(locks)
}

/// Resolve hidden flags conservatively. A hidden parent or cell style hides the
/// formula even where applyProtection inheritance is ambiguous.
pub fn hidden_styles(xml: &[u8]) -> Result<Vec<bool>> {
    #[derive(Default)]
    struct HiddenXf { parent: usize, hidden: Option<bool> }
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut parents = Vec::<HiddenXf>::new();
    let mut cells = Vec::<HiddenXf>::new();
    let mut active: Option<(bool, HiddenXf)> = None;
    let mut parent_seen = false;
    let mut cell_seen = false;
    let mut parent_count = None;
    let mut cell_count = None;
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.local_name().as_ref().to_vec();
                if stack.len() == 1 && stack[0] == b"styleSheet" && (name == b"cellStyleXfs" || name == b"cellXfs") {
                    let (seen, count) = if name == b"cellStyleXfs" { (&mut parent_seen, &mut parent_count) } else { (&mut cell_seen, &mut cell_count) };
                    ensure!(!*seen, "duplicate protection style table");
                    *seen = true;
                    *count = attribute(element, b"count")?.map(|value| value.parse::<usize>()).transpose()?;
                    ensure!(count.is_none_or(|count| count <= 65_536), "too many protection styles");
                }
                if name == b"xf" && stack.len() == 2 && stack[0] == b"styleSheet" && (stack[1] == b"cellStyleXfs" || stack[1] == b"cellXfs") {
                    ensure!(active.is_none(), "nested protection style");
                    let is_parent = stack[1] == b"cellStyleXfs";
                    attribute(element, b"applyProtection")?.map(|value| boolean(&value)).transpose()?;
                    let xf = HiddenXf { parent: attribute(element, b"xfId")?.map(|value| value.parse()).transpose()?.unwrap_or(0), hidden: None };
                    if matches!(event, Event::Empty(_)) { if is_parent { parents.push(xf); } else { cells.push(xf); } }
                    else { active = Some((is_parent, xf)); }
                } else if stack.len() == 2 && stack[0] == b"styleSheet" && (stack[1] == b"cellStyleXfs" || stack[1] == b"cellXfs") {
                    bail!("unsupported protection style table content");
                } else if name == b"protection" && stack.len() == 3 && stack[0] == b"styleSheet" && stack[2] == b"xf"
                    && (stack[1] == b"cellXfs" || stack[1] == b"cellStyleXfs") && let Some((_, xf)) = &mut active {
                    ensure!(xf.hidden.is_none(), "duplicate cell protection");
                    xf.hidden = Some(attribute(element, b"hidden")?.map(|value| boolean(&value)).transpose()?.unwrap_or(false));
                }
                if matches!(event, Event::Start(_)) { stack.push(name); }
            }
            Event::End(element) => {
                let name = element.local_name().as_ref().to_vec();
                ensure!(stack.pop().as_deref() == Some(name.as_slice()), "invalid protection style XML");
                if name == b"xf" && stack.len() == 2 && stack[0] == b"styleSheet" && (stack[1] == b"cellXfs" || stack[1] == b"cellStyleXfs") {
                    let (is_parent, xf) = active.take().ok_or_else(|| anyhow::anyhow!("invalid protection style"))?;
                    if is_parent { parents.push(xf); } else { cells.push(xf); }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        ensure!(parents.len() <= 65_536 && cells.len() <= 65_536, "too many protection styles");
    }
    ensure!(stack.is_empty() && active.is_none() && cell_seen && !cells.is_empty(), "invalid protection style table");
    ensure!(parent_count.is_none_or(|count| count == parents.len()) && cell_count.is_none_or(|count| count == cells.len()), "protection style count mismatch");
    cells.into_iter().map(|xf| {
        let inherited = if parents.is_empty() && xf.parent == 0 { false } else { parents.get(xf.parent).ok_or_else(|| anyhow::anyhow!("invalid parent protection style"))?.hidden.unwrap_or(false) };
        Ok(inherited || xf.hidden.unwrap_or(false))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respects_sheet_flag_and_rejects_ambiguous_protection() {
        assert!(!sheet_is_protected(br#"<worksheet><sheetData/></worksheet>"#).unwrap());
        assert!(!sheet_is_protected(br#"<worksheet><sheetProtection sheet="0"/></worksheet>"#).unwrap());
        assert!(!sheet_is_protected(br#"<worksheet><sheetProtection sheet="false"/></worksheet>"#).unwrap());
        assert!(sheet_is_protected(br#"<worksheet><sheetProtection sheet="1"/></worksheet>"#).unwrap());
        assert!(sheet_is_protected(br#"<worksheet><sheetProtection/></worksheet>"#).unwrap());
        assert!(sheet_is_protected(br#"<worksheet><sheetProtection sheet="maybe"/></worksheet>"#).is_err());
        assert!(sheet_is_protected(br#"<worksheet><sheetProtection/><sheetProtection/></worksheet>"#).is_err());
        assert!(!formatting_allowed(br#"<worksheet><sheetProtection sheet="1"/></worksheet>"#).unwrap());
        assert!(!formatting_allowed(br#"<worksheet><sheetProtection sheet="1" formatCells="1"/></worksheet>"#).unwrap());
        assert!(formatting_allowed(br#"<worksheet><sheetProtection sheet="1" formatCells="0"/></worksheet>"#).unwrap());
        assert!(formatting_allowed(br#"<worksheet><sheetProtection sheet="0" formatCells="1"/></worksheet>"#).unwrap());
    }

    #[test]
    fn resolves_parent_and_cell_protection_without_guessing() {
        let xml = br#"<styleSheet><cellStyleXfs><xf><protection locked="0"/></xf><xf/></cellStyleXfs><cellXfs><xf xfId="0"/><xf xfId="0" applyProtection="1"><protection locked="1"/></xf><xf xfId="1" applyProtection="1"><protection locked="false"/></xf><xf xfId="1"/></cellXfs></styleSheet>"#;
        assert_eq!(locked_styles(xml).unwrap(), vec![false, true, false, true]);
        let ambiguous = br#"<styleSheet><cellStyleXfs><xf/></cellStyleXfs><cellXfs><xf xfId="0" applyProtection="0"><protection locked="0"/></xf></cellXfs></styleSheet>"#;
        assert!(locked_styles(ambiguous).is_err());
        let bad_parent = br#"<styleSheet><cellStyleXfs><xf/></cellStyleXfs><cellXfs><xf xfId="4"/></cellXfs></styleSheet>"#;
        assert!(locked_styles(bad_parent).is_err());
        let duplicate = br#"<styleSheet><cellXfs><xf/></cellXfs><cellXfs><xf/></cellXfs></styleSheet>"#;
        assert!(locked_styles(duplicate).is_err());
        let invalid = br#"<styleSheet><cellXfs><xf applyProtection="1"><protection locked="maybe"/></xf></cellXfs></styleSheet>"#;
        assert!(locked_styles(invalid).is_err());
        let nested = br#"<styleSheet><cellXfs><xf applyProtection="1"><extLst><ext><xf><protection locked="0"/></xf></ext></extLst></xf></cellXfs></styleSheet>"#;
        assert_eq!(locked_styles(nested).unwrap(), vec![true]);
        let wrong_count = br#"<styleSheet><cellXfs count="1"><xf/><xf/></cellXfs></styleSheet>"#;
        assert!(locked_styles(wrong_count).is_err());
        let too_many = br#"<styleSheet><cellXfs count="65537"/></styleSheet>"#;
        assert!(locked_styles(too_many).is_err());
    }

    #[test]
    fn resolves_hidden_flags_conservatively() {
        let xml = br#"<styleSheet><cellStyleXfs count="2"><xf/><xf><protection hidden="1"/></xf></cellStyleXfs><cellXfs count="4"><xf/><xf><protection hidden="1"/></xf><xf xfId="1"/><xf xfId="1" applyProtection="0"><protection hidden="0"/></xf></cellXfs></styleSheet>"#;
        assert_eq!(hidden_styles(xml).unwrap(), vec![false, true, true, true]);
        let malformed = br#"<styleSheet><cellXfs><xf><protection hidden="invalid"/></xf></cellXfs></styleSheet>"#;
        assert!(hidden_styles(malformed).is_err());
        let duplicate = br#"<styleSheet><cellXfs><xf><protection/><protection hidden="1"/></xf></cellXfs></styleSheet>"#;
        assert!(hidden_styles(duplicate).is_err());
    }
}
