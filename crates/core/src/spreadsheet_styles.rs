//! Lossless number-format changes to an XLSX cell-style table.
use std::collections::{BTreeMap, BTreeSet};
use anyhow::{Result, bail, ensure};
use quick_xml::{Reader, Writer, events::{BytesStart, Event}};

/// Built-in Excel number formats. Custom format editing is a separate feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberFormat { General, Integer, Decimal2, GroupedInteger, GroupedDecimal2, Percent0, Percent2, Scientific, Date, DateTime }

impl NumberFormat {
    pub fn id(self) -> u32 {
        match self { Self::General => 0, Self::Integer => 1, Self::Decimal2 => 2, Self::GroupedInteger => 3, Self::GroupedDecimal2 => 4, Self::Percent0 => 9, Self::Percent2 => 10, Self::Scientific => 11, Self::Date => 14, Self::DateTime => 22 }
    }
}

pub fn validate_style_indices(xml: &[u8], indices: &BTreeSet<u32>) -> Result<()> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut found = false;
    let mut count = 0u32;
    let mut declared = None;
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.local_name().as_ref().to_vec();
                if stack.len() == 1 && stack[0] == b"styleSheet" && name == b"cellXfs" {
                    ensure!(!found, "duplicate cell style table");
                    found = true;
                    declared = attribute(element, b"count")?.map(|value| value.parse::<u32>()).transpose()?;
                } else if stack.len() == 2 && stack[0] == b"styleSheet" && stack[1] == b"cellXfs" {
                    ensure!(name == b"xf", "unsupported cell style table content");
                    count = count.checked_add(1).ok_or_else(|| anyhow::anyhow!("too many cell styles"))?;
                }
                if matches!(event, Event::Start(_)) { stack.push(name); }
            }
            Event::End(element) => ensure!(stack.pop().as_deref() == Some(element.local_name().as_ref()), "invalid style XML"),
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty() && found && count > 0 && count <= 65_490, "invalid cell style table");
    ensure!(declared.is_none_or(|value| value == count), "cell style count mismatch");
    ensure!(indices.iter().all(|value| *value < count), "invalid cell style index");
    Ok(())
}

fn attribute(element: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    let mut value = None;
    for attr in element.attributes() {
        let attr = attr?;
        if attr.key.as_ref() == name {
            ensure!(value.is_none(), "duplicate style attribute");
            value = Some(attr.unescape_value()?.into_owned());
        }
    }
    Ok(value)
}

fn formatted_xf(bytes: &[u8], number_format: u32) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(bytes);
    let event = reader.read_event()?;
    let (element, empty) = match &event {
        Event::Start(element) => (element, false),
        Event::Empty(element) => (element, true),
        _ => bail!("invalid cell style"),
    };
    ensure!(element.local_name().as_ref() == b"xf", "invalid cell style");
    let mut replacement = BytesStart::new(std::str::from_utf8(element.name().as_ref())?.to_owned());
    for attr in element.attributes() {
        let attr = attr?;
        if attr.key.as_ref() != b"numFmtId" && attr.key.as_ref() != b"applyNumberFormat" { replacement.push_attribute(attr); }
    }
    let number_format = number_format.to_string();
    replacement.push_attribute(("numFmtId", number_format.as_str()));
    replacement.push_attribute(("applyNumberFormat", "1"));
    let mut writer = Writer::new(Vec::new());
    if empty { writer.write_event(Event::Empty(replacement))?; }
    else { writer.write_event(Event::Start(replacement))?; }
    let mut result = writer.into_inner();
    result.extend_from_slice(&bytes[reader.buffer_position() as usize..]);
    Ok(result)
}

/// Clone each referenced cell XF with the new number format, keeping all
/// unrelated style attributes and children, and reuse identical XFs.
pub fn apply_number_format(xml: &[u8], source_indices: &BTreeSet<u32>, format: NumberFormat) -> Result<(Vec<u8>, BTreeMap<u32, u32>)> {
    if source_indices.is_empty() { return Ok((xml.to_vec(), BTreeMap::new())); }
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut opening = None;
    let mut opening_start = 0usize;
    let mut opening_end = 0usize;
    let mut closing_start = None;
    let mut active_xf = None;
    let mut xfs = Vec::<Vec<u8>>::new();
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event()?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.local_name().as_ref().to_vec();
                if name == b"cellXfs" && stack.len() == 1 && stack[0] == b"styleSheet" {
                    ensure!(opening.is_none() && matches!(event, Event::Start(_)), "invalid cell style table");
                    opening_start = start;
                    opening_end = reader.buffer_position() as usize;
                    opening = Some(element.clone().into_owned());
                } else if name == b"numFmt" && stack.len() == 2 && stack[0] == b"styleSheet" && stack[1] == b"numFmts" {
                    let id = attribute(element, b"numFmtId")?.ok_or_else(|| anyhow::anyhow!("number format id is missing"))?.parse::<u32>()?;
                    ensure!(id != format.id(), "built-in number format is overridden by this workbook");
                } else if stack.len() == 2 && stack[0] == b"styleSheet" && stack[1] == b"cellXfs" {
                    ensure!(name == b"xf" && active_xf.is_none(), "unsupported cell style table content");
                    if matches!(event, Event::Empty(_)) { xfs.push(xml[start..reader.buffer_position() as usize].to_vec()); }
                    else { active_xf = Some(start); }
                    ensure!(xfs.len() <= 65_490, "too many cell styles");
                }
                if matches!(event, Event::Start(_)) { stack.push(name); }
            }
            Event::End(element) => {
                let name = element.local_name().as_ref().to_vec();
                ensure!(stack.pop().as_deref() == Some(name.as_slice()), "invalid style XML");
                if name == b"xf" && stack.len() == 2 && stack[0] == b"styleSheet" && stack[1] == b"cellXfs" {
                    let begin = active_xf.take().ok_or_else(|| anyhow::anyhow!("invalid cell style"))?;
                    xfs.push(xml[begin..reader.buffer_position() as usize].to_vec());
                } else if name == b"cellXfs" && stack.len() == 1 && stack[0] == b"styleSheet" {
                    ensure!(closing_start.replace(start).is_none() && active_xf.is_none(), "invalid cell style table");
                }
            }
            Event::Text(text) if stack.len() == 2 && stack[0] == b"styleSheet" && stack[1] == b"cellXfs" => {
                ensure!(text.iter().all(u8::is_ascii_whitespace), "invalid cell style table text");
            }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty() && active_xf.is_none() && !xfs.is_empty() && xfs.len() <= 65_490, "incomplete or oversized cell style table");
    let opening = opening.ok_or_else(|| anyhow::anyhow!("cell style table is missing"))?;
    let closing_start = closing_start.ok_or_else(|| anyhow::anyhow!("cell style table is incomplete"))?;
    let count = attribute(&opening, b"count")?.map(|value| value.parse::<usize>()).transpose()?;
    ensure!(count.is_none_or(|count| count == xfs.len()), "cell style count mismatch");
    let mut lookup = BTreeMap::<Vec<u8>, u32>::new();
    for (index, xf) in xfs.iter().enumerate() { lookup.entry(xf.clone()).or_insert(index as u32); }
    let mut additions = Vec::new();
    let mut mapping = BTreeMap::new();
    for &index in source_indices {
        let source = xfs.get(index as usize).ok_or_else(|| anyhow::anyhow!("invalid cell style index"))?;
        let new_xf = formatted_xf(source, format.id())?;
        let target = if let Some(target) = lookup.get(&new_xf) { *target } else {
            ensure!(xfs.len() + additions.len() < 65_490, "too many cell styles");
            let target = (xfs.len() + additions.len()) as u32;
            lookup.insert(new_xf.clone(), target);
            additions.push(new_xf);
            target
        };
        mapping.insert(index, target);
    }
    if additions.is_empty() { return Ok((xml.to_vec(), mapping)); }
    let count = (xfs.len() + additions.len()).to_string();
    let mut replacement = BytesStart::new(std::str::from_utf8(opening.name().as_ref())?.to_owned());
    for attr in opening.attributes() { let attr = attr?; if attr.key.as_ref() != b"count" { replacement.push_attribute(attr); } }
    replacement.push_attribute(("count", count.as_str()));
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Start(replacement))?;
    let mut result = xml[..opening_start].to_vec();
    result.extend_from_slice(&writer.into_inner());
    result.extend_from_slice(&xml[opening_end..closing_start]);
    for xf in additions { result.extend_from_slice(&xf); }
    result.extend_from_slice(&xml[closing_start..]);
    Ok((result, mapping))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_number_format_without_changing_other_style_properties() {
        let xml = br#"<styleSheet><fonts count="1"><font/></fonts><cellXfs count="2"><xf numFmtId="0" fontId="0"/><xf numFmtId="4" fontId="3" borderId="2" applyAlignment="1"><alignment horizontal="center"/><protection locked="0"/></xf></cellXfs><cellStyles count="0"/></styleSheet>"#;
        let (updated, mapping) = apply_number_format(xml, &BTreeSet::from([0, 1]), NumberFormat::Percent2).unwrap();
        assert_eq!(mapping, BTreeMap::from([(0, 2), (1, 3)]));
        let text = std::str::from_utf8(&updated).unwrap();
        assert!(text.contains("<fonts count=\"1\"><font/></fonts>"));
        assert!(text.contains("<cellXfs count=\"4\">"));
        assert!(text.contains("<xf fontId=\"3\" borderId=\"2\" applyAlignment=\"1\" numFmtId=\"10\" applyNumberFormat=\"1\"><alignment horizontal=\"center\"/><protection locked=\"0\"/></xf>"));
        assert!(text.contains("<cellStyles count=\"0\"/>"));
        let (again, reused) = apply_number_format(&updated, &BTreeSet::from([2, 3]), NumberFormat::Percent2).unwrap();
        assert_eq!(again, updated);
        assert_eq!(reused, BTreeMap::from([(2, 2), (3, 3)]));
        assert!(apply_number_format(xml, &BTreeSet::from([5]), NumberFormat::General).is_err());
        let invalid = br#"<styleSheet><cellXfs count="1"><xf/><xf/></cellXfs></styleSheet>"#;
        assert!(apply_number_format(invalid, &BTreeSet::from([0]), NumberFormat::Integer).is_err());
        let overridden = br#"<styleSheet><numFmts count="1"><numFmt numFmtId="2" formatCode="custom"/></numFmts><cellXfs count="1"><xf/></cellXfs></styleSheet>"#;
        assert!(apply_number_format(overridden, &BTreeSet::from([0]), NumberFormat::Decimal2).is_err());
        assert!(apply_number_format(overridden, &BTreeSet::from([0]), NumberFormat::Integer).is_ok());
    }
}
