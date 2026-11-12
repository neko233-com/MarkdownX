//! Targeted worksheet XML edits. This layer does not write files or ZIP archives.
use std::collections::{BTreeMap, BTreeSet};
use anyhow::{Result, bail, ensure};
use quick_xml::{Reader, Writer, events::{BytesStart, Event}};
use crate::spreadsheet_edit::CellInput;

/// Replace cells and insert missing cells into existing rows.
/// Shared/array formulas and unrecognized cell children are rejected.
pub fn patch_existing_cells(xml: &[u8], edits: &BTreeMap<(u32, u32), CellInput>) -> Result<Vec<u8>> {
    if edits.is_empty() { return Ok(xml.to_vec()); }
    validate_ranges(xml, edits)?;
    let mut targets = BTreeMap::new();
    for (&(row, col), value) in edits {
        ensure!(row < 1_048_576 && col < 16_384, "cell outside worksheet limits");
        targets.insert(cell_ref(row, col), value);
    }
    let mut reader = Reader::from_reader(xml);
    let mut output = Vec::new();
    let mut written = 0usize;
    let mut stack = Vec::<Vec<u8>>::new();
    let mut found = BTreeSet::new();
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event()?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) if e.local_name().as_ref() == b"c" && stack.last().is_some_and(|v| v == b"row") && stack.iter().any(|v| v == b"sheetData") => {
                let mut reference = None;
                let mut opening = BytesStart::new(std::str::from_utf8(e.name().as_ref())?.to_owned());
                for attr in e.attributes() {
                    let attr = attr?;
                    if attr.key.as_ref() == b"r" { reference = Some(attr.unescape_value()?.into_owned()); }
                    if attr.key.as_ref() != b"t" { opening.push_attribute(attr); }
                }
                if let Some((reference, value)) = reference.as_ref().and_then(|r| targets.get(r).map(|v| (r, *v))) {
                    ensure!(e.name().as_ref() == b"c", "prefixed worksheet cells are not supported yet");
                    for attr in e.attributes() {
                        let attr = attr?;
                        ensure!([b"r".as_slice(), b"s", b"t"].contains(&attr.key.as_ref()), "cell metadata cannot be edited safely");
                    }
                    ensure!(found.insert(reference.clone()), "duplicate worksheet cell");
                    let empty = matches!(event, Event::Empty(_));
                    if !empty { consume_cell(&mut reader)?; }
                    let end = reader.buffer_position() as usize;
                    output.extend_from_slice(&xml[written..start]);
                    output.extend_from_slice(&encode_cell(opening, value)?);
                    written = end;
                } else if matches!(event, Event::Start(_)) { stack.push(e.local_name().as_ref().to_vec()); }
            }
            Event::Start(ref e) => stack.push(e.local_name().as_ref().to_vec()),
            Event::End(_) => { ensure!(stack.pop().is_some(), "unexpected closing element"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty(), "incomplete worksheet XML");
    output.extend_from_slice(&xml[written..]);
    let missing = edits.iter().filter_map(|(&(r, c), value)| {
        if found.contains(&cell_ref(r, c)) || matches!(value, CellInput::Clear) { None } else { Some(((r, c), value.clone())) }
    }).collect::<BTreeMap<_, _>>();
    if missing.is_empty() { return Ok(output); }
    insert_into_rows(&output, &missing)
}

/// Change only cell style indices, preserving the original value, formula,
/// extension children and other attributes byte-for-byte.
pub(crate) fn patch_style_indices(xml: &[u8], styles: &BTreeMap<(u32, u32), u32>) -> Result<Vec<u8>> {
    if styles.is_empty() { return Ok(xml.to_vec()); }
    ensure!(styles.len() <= 100_000, "too many style edits");
    let targets = styles.keys().copied().collect::<BTreeSet<_>>();
    ensure!(targets.iter().all(|(row, column)| *row < 1_048_576 && *column < 16_384), "cell outside worksheet limits");
    effective_styles(xml, &targets)?;
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut output = Vec::new();
    let mut written = 0usize;
    let mut found = BTreeSet::new();
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event()?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.local_name().as_ref().to_vec();
                let in_row = stack.len() == 3 && stack[0] == b"worksheet" && stack[1] == b"sheetData" && stack[2] == b"row";
                if name == b"c" && in_row {
                    let reference = attribute(element, b"r")?.ok_or_else(|| anyhow::anyhow!("cell has no reference"))?;
                    let address = parse_cell_ref(&reference)?;
                    if let Some(style) = styles.get(&address) {
                        ensure!(element.name().as_ref() == b"c" && found.insert(address), "unsupported or duplicate worksheet cell");
                        let mut replacement = BytesStart::new("c");
                        for attr in element.attributes() { let attr = attr?; if attr.key.as_ref() != b"s" { replacement.push_attribute(attr); } }
                        let style = style.to_string();
                        replacement.push_attribute(("s", style.as_str()));
                        let mut writer = Writer::new(Vec::new());
                        if matches!(event, Event::Empty(_)) { writer.write_event(Event::Empty(replacement))?; }
                        else { writer.write_event(Event::Start(replacement))?; }
                        output.extend_from_slice(&xml[written..start]);
                        output.extend_from_slice(&writer.into_inner());
                        written = reader.buffer_position() as usize;
                    }
                }
                if matches!(event, Event::Start(_)) { stack.push(name); }
            }
            Event::End(element) => { ensure!(stack.pop().as_deref() == Some(element.local_name().as_ref()), "invalid worksheet XML"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty(), "incomplete worksheet XML");
    output.extend_from_slice(&xml[written..]);
    let missing = targets.difference(&found).copied().collect::<BTreeSet<_>>();
    if missing.is_empty() { return Ok(output); }
    let blanks = missing.iter().copied().map(|address| (address, CellInput::Clear)).collect::<BTreeMap<_, _>>();
    let with_blanks = insert_into_rows(&output, &blanks)?;
    let missing_styles = missing.into_iter().map(|address| (address, styles[&address])).collect::<BTreeMap<_, _>>();
    patch_style_indices(&with_blanks, &missing_styles)
}

// Editing a non-anchor merged cell can create content that Excel hides or
// discards. Array/data-table formula ranges must remain a single operation.
fn validate_ranges(xml: &[u8], edits: &BTreeMap<(u32, u32), CellInput>) -> Result<()> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.local_name();
                let merged = name.as_ref() == b"mergeCell" && stack.last().is_some_and(|p| p == b"mergeCells");
                let array = name.as_ref() == b"f" && stack.last().is_some_and(|p| p == b"c")
                    && attribute(e, b"t")?.is_some_and(|kind| kind == "array" || kind == "dataTable");
                if merged || array {
                    let reference = attribute(e, b"ref")?.ok_or_else(|| anyhow::anyhow!("protected range has no reference"))?;
                    let (first, last) = reference.split_once(':').unwrap_or((&reference, &reference));
                    let (top, left) = parse_cell_ref(first)?;
                    let (bottom, right) = parse_cell_ref(last)?;
                    ensure!(top <= bottom && left <= right, "invalid protected cell range");
                    for (&(row, col), _) in edits.range((top, left)..=(bottom, right)) {
                        if (left..=right).contains(&col) {
                            ensure!(merged && (row, col) == (top, left), "cannot safely edit part of a merged or array-formula range");
                        }
                    }
                }
                if matches!(event, Event::Start(_)) { stack.push(name.as_ref().to_vec()); }
            }
            Event::End(_) => { ensure!(stack.pop().is_some(), "unexpected closing element"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty(), "incomplete worksheet XML");
    Ok(())
}

/// Insert new cells and rows without rewriting unrelated worksheet bytes.
fn insert_into_rows(xml: &[u8], missing: &BTreeMap<(u32, u32), CellInput>) -> Result<Vec<u8>> {
    struct Row { index: u32, start: usize, end: usize, cells: Vec<(u32, usize)>, empty: Option<BytesStart<'static>>, style: Option<u32> }
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut rows = Vec::<Row>::new();
    let mut columns = Vec::<(u32, u32, u32)>::new();
    let mut current: Option<Row> = None;
    let mut sheet_end = None;
    let mut empty_sheet = None;
    loop {
        let position = reader.buffer_position() as usize;
        let event = reader.read_event()?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) if e.local_name().as_ref() == b"col" && stack.last().is_some_and(|p| p == b"cols") => {
                ensure!(e.name().as_ref() == b"col", "prefixed column styles are unsupported");
                let first = attribute(e, b"min")?.ok_or_else(|| anyhow::anyhow!("column has no minimum"))?.parse::<u32>()?;
                let last = attribute(e, b"max")?.ok_or_else(|| anyhow::anyhow!("column has no maximum"))?.parse::<u32>()?;
                ensure!((1..=16_384).contains(&first) && (first..=16_384).contains(&last), "invalid column style range");
                let style = attribute(e, b"style")?.map(|s| s.parse::<u32>()).transpose()?.unwrap_or(0);
                columns.push((first - 1, last - 1, style));
                if matches!(event, Event::Start(_)) { stack.push(b"col".to_vec()); }
            }
            Event::Empty(e) if e.local_name().as_ref() == b"sheetData" && stack.last().is_some_and(|p| p == b"worksheet") => {
                ensure!(e.name().as_ref() == b"sheetData" && empty_sheet.is_none() && sheet_end.is_none(), "unsupported sheet data");
                empty_sheet = Some((position, reader.buffer_position() as usize, e.into_owned()));
            }
            Event::Start(e) if e.local_name().as_ref() == b"sheetData" && stack.last().is_some_and(|p| p == b"worksheet") => {
                ensure!(e.name().as_ref() == b"sheetData" && empty_sheet.is_none() && sheet_end.is_none(), "unsupported sheet data");
                stack.push(b"sheetData".to_vec());
            }
            Event::End(e) if e.local_name().as_ref() == b"sheetData" && stack.last().is_some_and(|p| p == b"sheetData") => {
                ensure!(sheet_end.is_none(), "duplicate sheet data");
                sheet_end = Some(position);
                stack.pop();
            }
            Event::Start(e) if e.local_name().as_ref() == b"row" && stack.last().is_some_and(|p| p == b"sheetData") => {
                ensure!(e.name().as_ref() == b"row" && current.is_none(), "unsupported row structure");
                let row = attribute(&e, b"r")?.ok_or_else(|| anyhow::anyhow!("row has no index"))?.parse::<u32>()?;
                ensure!((1..=1_048_576).contains(&row), "invalid row index");
                let style = row_style(&e)?;
                current = Some(Row { index: row - 1, start: position, end: 0, cells: Vec::new(), empty: None, style });
                stack.push(b"row".to_vec());
            }
            Event::Empty(e) if e.local_name().as_ref() == b"row" && stack.last().is_some_and(|p| p == b"sheetData") => {
                ensure!(e.name().as_ref() == b"row" && current.is_none(), "unsupported row structure");
                let row = attribute(&e, b"r")?.ok_or_else(|| anyhow::anyhow!("row has no index"))?.parse::<u32>()?;
                ensure!((1..=1_048_576).contains(&row) && rows.last().is_none_or(|old| old.index < row - 1), "invalid worksheet row order");
                let style = row_style(&e)?;
                rows.push(Row { index: row - 1, start: position, end: reader.buffer_position() as usize, cells: Vec::new(), empty: Some(e.into_owned()), style });
            }
            Event::Start(ref e) | Event::Empty(ref e) if e.local_name().as_ref() == b"c" && stack.last().is_some_and(|p| p == b"row") => {
                ensure!(e.name().as_ref() == b"c", "prefixed worksheet cells are not supported yet");
                let reference = attribute(e, b"r")?.ok_or_else(|| anyhow::anyhow!("cell has no reference"))?;
                let (row, col) = parse_cell_ref(&reference)?;
                let current = current.as_mut().ok_or_else(|| anyhow::anyhow!("cell outside row"))?;
                ensure!(row == current.index && current.cells.last().is_none_or(|(old, _)| *old < col), "unsorted or invalid worksheet cells");
                current.cells.push((col, position));
                if matches!(event, Event::Start(_)) { stack.push(b"c".to_vec()); }
            }
            Event::End(e) if e.local_name().as_ref() == b"row" && stack.last().is_some_and(|p| p == b"row") => {
                let mut row = current.take().ok_or_else(|| anyhow::anyhow!("unexpected row end"))?;
                row.end = position;
                ensure!(rows.last().is_none_or(|old| old.index < row.index), "unsorted worksheet rows");
                rows.push(row);
                stack.pop();
            }
            Event::Start(e) => stack.push(e.local_name().as_ref().to_vec()),
            Event::End(_) => { ensure!(stack.pop().is_some(), "unexpected closing element"); }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty() && current.is_none(), "incomplete worksheet XML");
    let mut grouped = BTreeMap::<u32, Vec<(u32, &CellInput)>>::new();
    for (&(row, col), value) in missing { grouped.entry(row).or_default().push((col, value)); }
    if let Some((start, end, opening)) = empty_sheet {
        ensure!(rows.is_empty() && sheet_end.is_none(), "invalid empty sheet data");
        let mut inner = Vec::new();
        for (row, cells) in grouped { inner.extend_from_slice(&encode_row(row, &cells, None, None, &columns)?); }
        let mut result = xml[..start].to_vec();
        result.extend_from_slice(&wrap_element(opening, &inner)?);
        result.extend_from_slice(&xml[end..]);
        return update_dimension(&result, missing.keys().copied());
    }
    let sheet_end = sheet_end.ok_or_else(|| anyhow::anyhow!("worksheet has no sheet data"))?;
    let mut insertions = BTreeMap::<usize, Vec<u8>>::new();
    let mut replacements = BTreeMap::<usize, (usize, Vec<u8>)>::new();
    for (row, cells) in grouped {
        if let Ok(index) = rows.binary_search_by_key(&row, |r| r.index) {
            let row_data = &rows[index];
            if let Some(opening) = &row_data.empty {
                replacements.insert(row_data.start, (row_data.end, encode_row(row, &cells, Some(opening.clone()), row_data.style, &columns)?));
                continue;
            }
            for (col, value) in cells {
                let index = row_data.cells.partition_point(|(c, _)| *c < col);
                let position = row_data.cells.get(index).map_or(row_data.end, |(_, pos)| *pos);
                insertions.entry(position).or_default().extend_from_slice(&encode_new_cell(row, col, value, inherited_style(row_data.style, &columns, col))?);
            }
        } else {
            let index = rows.partition_point(|r| r.index < row);
            let position = rows.get(index).map_or(sheet_end, |r| r.start);
            insertions.entry(position).or_default().extend_from_slice(&encode_row(row, &cells, None, None, &columns)?);
        }
    }
    let mut output = Vec::new();
    let mut written = 0;
    let positions = insertions.keys().chain(replacements.keys()).copied().collect::<BTreeSet<_>>();
    for position in positions {
        ensure!(position >= written, "overlapping worksheet changes");
        output.extend_from_slice(&xml[written..position]);
        written = position;
        if let Some(data) = insertions.get(&position) { output.extend_from_slice(data); }
        if let Some((end, data)) = replacements.get(&position) { output.extend_from_slice(data); written = *end; }
    }
    output.extend_from_slice(&xml[written..]);
    update_dimension(&output, missing.keys().copied())
}

fn row_style(row: &BytesStart<'_>) -> Result<Option<u32>> {
    let custom = attribute(row, b"customFormat")?.unwrap_or_else(|| "0".into());
    ensure!(["0", "1", "true", "false"].iter().any(|v| custom.eq_ignore_ascii_case(v)), "invalid row custom format");
    if custom == "1" || custom.eq_ignore_ascii_case("true") { Ok(Some(attribute(row, b"s")?.map(|s| s.parse::<u32>()).transpose()?.unwrap_or(0))) }
    else { Ok(None) }
}

/// Resolve effective cell-style indices without allocating a rectangular grid.
/// Existing cells use their own style (zero when absent); blank cells inherit
/// the row style, then the first applicable column declaration.
pub(crate) fn effective_styles(xml: &[u8], targets: &BTreeSet<(u32, u32)>) -> Result<BTreeMap<(u32, u32), (u32, Option<u32>)>> {
    let mut reader = Reader::from_reader(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut current_row = None;
    let mut rows = BTreeMap::<u32, u32>::new();
    let target_rows = targets.iter().map(|(row, _)| *row).collect::<BTreeSet<_>>();
    let mut explicit = BTreeMap::<(u32, u32), (u32, bool)>::new();
    let mut columns = Vec::<(u32, u32, u32)>::new();
    let mut seen_rows = BTreeSet::new();
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.local_name().as_ref().to_vec();
                let parent = stack.last().map(Vec::as_slice);
                let in_columns = stack.len() == 2 && stack[0] == b"worksheet" && parent == Some(b"cols");
                let in_data = stack.len() == 2 && stack[0] == b"worksheet" && parent == Some(b"sheetData");
                let in_row = stack.len() == 3 && stack[0] == b"worksheet" && stack[1] == b"sheetData" && parent == Some(b"row");
                if name == b"col" && in_columns {
                    let first = attribute(element, b"min")?.ok_or_else(|| anyhow::anyhow!("column has no minimum"))?.parse::<u32>()?;
                    let last = attribute(element, b"max")?.ok_or_else(|| anyhow::anyhow!("column has no maximum"))?.parse::<u32>()?;
                    ensure!((1..=16_384).contains(&first) && (first..=16_384).contains(&last), "invalid column style range");
                    let style = attribute(element, b"style")?.map(|value| value.parse::<u32>()).transpose()?.unwrap_or(0);
                    ensure!(columns.len() < 100_000, "too many column style ranges");
                    columns.push((first - 1, last - 1, style));
                } else if name == b"row" && in_data {
                    ensure!(current_row.is_none(), "nested worksheet row");
                    let row = attribute(element, b"r")?.ok_or_else(|| anyhow::anyhow!("row has no index"))?.parse::<u32>()?;
                    ensure!((1..=1_048_576).contains(&row) && seen_rows.insert(row), "invalid or duplicate worksheet row");
                    let row = row - 1;
                    if target_rows.contains(&row) && let Some(style) = row_style(element)? { rows.insert(row, style); }
                    if matches!(event, Event::Start(_)) { current_row = Some(row); }
                } else if name == b"c" && in_row {
                    let row = current_row.ok_or_else(|| anyhow::anyhow!("cell outside row"))?;
                    let reference = attribute(element, b"r")?.ok_or_else(|| anyhow::anyhow!("cell has no reference"))?;
                    let address = parse_cell_ref(&reference)?;
                    ensure!(address.0 == row, "cell is in the wrong row");
                    if targets.contains(&address) {
                        let style = attribute(element, b"s")?.map(|value| value.parse::<u32>()).transpose()?;
                        ensure!(explicit.insert(address, (style.unwrap_or(0), style.is_some())).is_none(), "duplicate worksheet cell");
                    }
                }
                if matches!(event, Event::Start(_)) { stack.push(name); }
            }
            Event::End(element) => {
                let name = element.local_name().as_ref().to_vec();
                ensure!(stack.pop().as_deref() == Some(name.as_slice()), "invalid worksheet XML");
                if name == b"row" && stack.len() == 2 && stack[0] == b"worksheet" && stack[1] == b"sheetData" { current_row = None; }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.is_empty() && current_row.is_none(), "incomplete worksheet XML");
    const LIMIT: usize = 16_384;
    fn next_unassigned(parent: &mut [usize], mut index: usize) -> usize {
        let mut root = index;
        while parent[root] != root { root = parent[root]; }
        while parent[index] != index { let next = parent[index]; parent[index] = root; index = next; }
        root
    }
    let mut parent = (0..=LIMIT).collect::<Vec<_>>();
    let mut column_styles = vec![None; LIMIT];
    for (first, last, style) in columns {
        let mut index = next_unassigned(&mut parent, first as usize);
        while index <= last as usize {
            column_styles[index] = Some(style);
            let next = next_unassigned(&mut parent, index + 1);
            parent[index] = next;
            index = next;
        }
    }
    let mut result = BTreeMap::new();
    for &(row, column) in targets {
        ensure!(row < 1_048_576 && column < 16_384, "cell outside worksheet limits");
        let inherited = rows.get(&row).copied().or(column_styles[column as usize]).unwrap_or(0);
        // An omitted cell style is read as index zero by the calculation
        // engine. Also check the inherited style to avoid bypassing a locked
        // row or column when another producer interprets omission differently.
        let resolved = match explicit.get(&(row, column)).copied() {
            Some((style, false)) if inherited != style => (style, Some(inherited)),
            Some((style, _)) => (style, None),
            None => (inherited, None),
        };
        result.insert((row, column), resolved);
    }
    Ok(result)
}

fn inherited_style(row: Option<u32>, columns: &[(u32, u32, u32)], column: u32) -> Option<u32> {
    row.or_else(|| columns.iter().find(|(first, last, _)| (*first..=*last).contains(&column)).map(|(_, _, style)| *style)).filter(|style| *style != 0)
}

fn encode_new_cell(row: u32, col: u32, value: &CellInput, style: Option<u32>) -> Result<Vec<u8>> {
    let reference = cell_ref(row, col);
    let mut opening = BytesStart::new("c");
    opening.push_attribute(("r", reference.as_str()));
    let style = style.map(|value| value.to_string());
    if let Some(style) = &style { opening.push_attribute(("s", style.as_str())); }
    encode_cell(opening, value)
}

fn encode_row(row: u32, cells: &[(u32, &CellInput)], opening: Option<BytesStart<'static>>, style: Option<u32>, columns: &[(u32, u32, u32)]) -> Result<Vec<u8>> {
    let mut opening = opening.unwrap_or_else(|| BytesStart::new("row"));
    let row_number = (row + 1).to_string();
    if attribute(&opening, b"r")?.is_none() { opening.push_attribute(("r", row_number.as_str())); }
    let mut inner = Vec::new();
    for &(col, value) in cells { inner.extend_from_slice(&encode_new_cell(row, col, value, inherited_style(style, columns, col))?); }
    wrap_element(opening, &inner)
}

fn wrap_element(opening: BytesStart<'_>, inner: &[u8]) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Start(opening.clone()))?;
    writer.get_mut().extend_from_slice(inner);
    writer.write_event(Event::End(opening.to_end()))?;
    Ok(writer.into_inner())
}

fn attribute(e: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    for attr in e.attributes() { let attr = attr?; if attr.key.as_ref() == name { return Ok(Some(attr.unescape_value()?.into_owned())); } }
    Ok(None)
}

fn parse_cell_ref(reference: &str) -> Result<(u32, u32)> {
    let split = reference.bytes().take_while(u8::is_ascii_uppercase).count();
    ensure!(split > 0 && split < reference.len(), "invalid cell reference");
    let mut col = 0u32;
    for byte in reference[..split].bytes() { col = col.checked_mul(26).and_then(|n| n.checked_add(u32::from(byte - b'A' + 1))).ok_or_else(|| anyhow::anyhow!("invalid column"))?; }
    let row: u32 = reference[split..].parse()?;
    ensure!((1..=1_048_576).contains(&row) && (1..=16_384).contains(&col), "cell outside worksheet limits");
    Ok((row - 1, col - 1))
}

fn update_dimension(xml: &[u8], cells: impl Iterator<Item = (u32, u32)>) -> Result<Vec<u8>> {
    let new_cells = cells.collect::<Vec<_>>();
    let mut reader = Reader::from_reader(xml);
    loop {
        let start = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Empty(e) if e.local_name().as_ref() == b"dimension" => {
                ensure!(e.name().as_ref() == b"dimension", "prefixed dimension is not supported");
                let old = attribute(&e, b"ref")?.ok_or_else(|| anyhow::anyhow!("dimension has no range"))?;
                let mut parts = old.split(':');
                let first = parse_cell_ref(parts.next().unwrap_or_default())?;
                let last = match parts.next() { Some(last) => parse_cell_ref(last)?, None => first };
                ensure!(parts.next().is_none() && first.0 <= last.0 && first.1 <= last.1, "invalid dimension");
                let mut min = first;
                let mut max = last;
                for &(row, col) in &new_cells { min.0 = min.0.min(row); min.1 = min.1.min(col); max.0 = max.0.max(row); max.1 = max.1.max(col); }
                let range = if min == max { cell_ref(min.0, min.1) } else { format!("{}:{}", cell_ref(min.0, min.1), cell_ref(max.0, max.1)) };
                let mut replacement = BytesStart::new("dimension");
                for attr in e.attributes() { let attr = attr?; if attr.key.as_ref() != b"ref" { replacement.push_attribute(attr); } }
                replacement.push_attribute(("ref", range.as_str()));
                let mut writer = Writer::new(Vec::new());
                writer.write_event(Event::Empty(replacement))?;
                let mut result = xml[..start].to_vec();
                result.extend_from_slice(&writer.into_inner());
                result.extend_from_slice(&xml[reader.buffer_position() as usize..]);
                return Ok(result);
            }
            Event::Start(e) if e.local_name().as_ref() == b"dimension" => bail!("unsupported dimension element"),
            Event::Eof => return Ok(xml.to_vec()),
            _ => {}
        }
    }
}

fn consume_cell(reader: &mut Reader<&[u8]>) -> Result<()> {
    let mut depth = 1usize;
    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                if depth == 1 {
                    ensure!([b"v".as_slice(), b"f", b"is"].contains(&e.local_name().as_ref()), "unsupported cell content");
                    if e.local_name().as_ref() == b"f" { ensure!(e.attributes().next().is_none(), "shared or array formula cannot be edited safely"); }
                }
                depth += 1;
            }
            Event::Empty(e) if depth == 1 => {
                ensure!([b"v".as_slice(), b"f", b"is"].contains(&e.local_name().as_ref()), "unsupported cell content");
                if e.local_name().as_ref() == b"f" { ensure!(e.attributes().next().is_none(), "shared or array formula cannot be edited safely"); }
            }
            Event::End(_) => { depth -= 1; if depth == 0 { return Ok(()); } }
            Event::Eof => bail!("incomplete worksheet cell"),
            Event::Comment(_) | Event::PI(_) if depth == 1 => bail!("unsupported cell content"),
            _ => {}
        }
    }
}

fn encode_cell(mut opening: BytesStart<'_>, value: &CellInput) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    match value {
        CellInput::Text(text) => {
            ensure!(valid_xml_text(text) && text.chars().count() <= 32_767, "invalid cell text");
            opening.push_attribute(("t", "inlineStr"));
            writer.write_event(Event::Start(opening.clone()))?;
            let escaped = quick_xml::escape::escape(text);
            writer.get_mut().extend_from_slice(format!("<is><t xml:space=\"preserve\">{escaped}</t></is>").as_bytes());
        }
        CellInput::Number(number) => {
            ensure!(number.is_finite(), "invalid cell number");
            writer.write_event(Event::Start(opening.clone()))?;
            writer.get_mut().extend_from_slice(format!("<v>{number}</v>").as_bytes());
        }
        CellInput::Boolean(value) => {
            opening.push_attribute(("t", "b"));
            writer.write_event(Event::Start(opening.clone()))?;
            writer.get_mut().extend_from_slice(if *value { b"<v>1</v>" } else { b"<v>0</v>" });
        }
        CellInput::Formula(formula) => {
            ensure!(!formula.trim().is_empty() && !formula.starts_with('=') && formula.chars().count() <= 8_192 && valid_xml_text(formula), "invalid formula");
            writer.write_event(Event::Start(opening.clone()))?;
            let escaped = quick_xml::escape::escape(formula);
            writer.get_mut().extend_from_slice(format!("<f>{escaped}</f>").as_bytes());
        }
        CellInput::Clear => { writer.write_event(Event::Empty(opening))?; return Ok(writer.into_inner()); }
    }
    writer.write_event(Event::End(opening.to_end()))?;
    Ok(writer.into_inner())
}

fn valid_xml_text(s: &str) -> bool {
    s.chars().all(|c| matches!(c, '\u{9}' | '\u{a}' | '\u{d}' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
}

fn cell_ref(row: u32, mut col: u32) -> String {
    let mut result = String::new();
    loop { result.insert(0, (b'A' + (col % 26) as u8) as char); if col < 26 { break; } col = col / 26 - 1; }
    format!("{result}{}", row + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_changes_preserve_cell_content_and_insert_styled_blanks() {
        let xml = br#"<worksheet><dimension ref="A1:C1"/><sheetData><row r="1"><c r="A1" s="2" vm="7"><f t="shared" si="3">A2+1</f><v>9</v><extLst><ext uri="x"/></extLst></c><c r="C1" t="inlineStr"><is><t>Text &amp; more</t></is></c></row></sheetData></worksheet>"#;
        let styles = BTreeMap::from([((0, 0), 4), ((0, 1), 5), ((1, 0), 0)]);
        let result = String::from_utf8(patch_style_indices(xml, &styles).unwrap()).unwrap();
        assert!(result.contains("<c r=\"A1\" vm=\"7\" s=\"4\"><f t=\"shared\" si=\"3\">A2+1</f><v>9</v><extLst><ext uri=\"x\"/></extLst></c>"));
        assert!(result.contains("<c r=\"B1\" s=\"5\"/>"));
        assert!(result.contains("<c r=\"C1\" t=\"inlineStr\"><is><t>Text &amp; more</t></is></c>"));
        assert!(result.contains("<c r=\"A2\" s=\"0\"/>"));
        assert!(result.contains("dimension ref=\"A1:C2\""));
        assert!(patch_style_indices(xml, &BTreeMap::from([((1_048_576, 0), 1)])).is_err());
    }

    #[test]
    fn protected_cell_styles_follow_cell_row_and_first_column_precedence() {
        let xml = br#"<worksheet><cols><col min="2" max="4" style="2"/><col min="3" max="5" style="3"/></cols><sheetData><row r="2" customFormat="1" s="4"><c r="B2"/><c r="C2" s="5"/></row><row r="4" customFormat="1" s="6"/></sheetData></worksheet>"#;
        let targets = BTreeSet::from([(0, 0), (0, 1), (0, 2), (0, 4), (1, 0), (1, 1), (1, 2), (1, 3), (3, 0), (1_048_575, 16_383)]);
        let styles = effective_styles(xml, &targets).unwrap();
        assert_eq!(styles[&(0, 0)], (0, None));
        assert_eq!(styles[&(0, 1)], (2, None));
        assert_eq!(styles[&(0, 2)], (2, None));
        assert_eq!(styles[&(0, 4)], (3, None));
        assert_eq!(styles[&(1, 0)], (4, None));
        assert_eq!(styles[&(1, 1)], (0, Some(4)));
        assert_eq!(styles[&(1, 2)], (5, None));
        assert_eq!(styles[&(1, 3)], (4, None));
        assert_eq!(styles[&(3, 0)], (6, None));
        assert_eq!(styles[&(1_048_575, 16_383)], (0, None));
        let duplicate = br#"<worksheet><sheetData><row r="1"/><row r="1"/></sheetData></worksheet>"#;
        assert!(effective_styles(duplicate, &targets).is_err());
        let wrong_row = br#"<worksheet><sheetData><row r="1"><c r="A2"/></row></sheetData></worksheet>"#;
        assert!(effective_styles(wrong_row, &targets).is_err());
        let nested = br#"<worksheet><ext><cols><col min="1" max="1" style="9"/></cols></ext><sheetData/></worksheet>"#;
        assert_eq!(effective_styles(nested, &BTreeSet::from([(0, 0)])).unwrap()[&(0, 0)], (0, None));
    }

    #[test]
    fn protected_style_lookup_handles_large_overlapping_column_ranges() {
        let mut xml = String::from("<worksheet><cols>");
        for _ in 0..16_384 { xml.push_str("<col min=\"1\" max=\"16384\" style=\"1\"/>"); }
        xml.push_str("</cols><sheetData/></worksheet>");
        let targets = (0..100_000).map(|index| (index / 16_384, index % 16_384)).collect::<BTreeSet<_>>();
        let styles = effective_styles(xml.as_bytes(), &targets).unwrap();
        assert_eq!(styles.len(), 100_000);
        assert!(styles.values().all(|style| *style == (1, None)));
    }

    #[test]
    fn protects_merged_cells_and_array_formula_ranges() {
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1"><v>4</v></c><c r="D1"><f t="array" ref="D1:E2">D3:E4</f><v>1</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="A1:B2"/></mergeCells></worksheet>"#;
        let change = |row, column| BTreeMap::from([((row, column), CellInput::Number(8.0))]);
        assert!(patch_existing_cells(xml, &change(0, 0)).is_ok());
        assert!(patch_existing_cells(xml, &change(1, 1)).is_err());
        assert!(patch_existing_cells(xml, &change(0, 3)).is_err());
        assert!(patch_existing_cells(xml, &change(1, 4)).is_err());
        assert!(patch_existing_cells(xml, &change(2, 2)).is_ok());
        let mut batch = change(2, 2);
        batch.extend(change(1, 0));
        assert!(patch_existing_cells(xml, &batch).is_err());
        let malformed = br#"<worksheet><sheetData/><mergeCells><mergeCell ref="B2:A1"/></mergeCells></worksheet>"#;
        assert!(patch_existing_cells(malformed, &change(0, 0)).is_err());
        assert_eq!(patch_existing_cells(malformed, &BTreeMap::new()).unwrap(), malformed);
    }

    #[test]
    fn rejects_editing_cells_with_unsupported_value_metadata() {
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1" vm="1"><v>42</v></c><c r="B1"><v>8</v></c></row></sheetData></worksheet>"#;
        assert!(patch_existing_cells(xml, &BTreeMap::from([((0, 0), CellInput::Number(1.0))])).is_err());
        let output = patch_existing_cells(xml, &BTreeMap::from([((0, 1), CellInput::Number(2.0))])).unwrap();
        assert!(String::from_utf8(output).unwrap().contains("<c r=\"A1\" vm=\"1\"><v>42</v></c>"));
    }

    #[test]
    fn changes_only_target_cell_and_keeps_styles() {
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1" s="4" t="s"><v>2</v></c><c r="B1"><v>9</v></c></row></sheetData><tail x="kept"/></worksheet>"#;
        let result = patch_existing_cells(xml, &BTreeMap::from([((0, 0), CellInput::Text(" 生命 & 值 ".into()))])).unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "<worksheet><sheetData><row r=\"1\"><c r=\"A1\" s=\"4\" t=\"inlineStr\"><is><t xml:space=\"preserve\"> 生命 &amp; 值 </t></is></c><c r=\"B1\"><v>9</v></c></row></sheetData><tail x=\"kept\"/></worksheet>");
    }

    #[test]
    fn refuses_missing_or_shared_formula_cells() {
        let xml = br#"<worksheet><sheetData><row><c r="A1"><f t="shared" si="0">B1</f><v>2</v></c></row></sheetData></worksheet>"#;
        assert!(patch_existing_cells(xml, &BTreeMap::from([((0, 0), CellInput::Number(3.0))])).is_err());
        assert_eq!(patch_existing_cells(xml, &BTreeMap::from([((0, 1), CellInput::Clear)])).unwrap(), xml);
        assert!(patch_existing_cells(xml, &BTreeMap::from([((0, 0), CellInput::Text("\0".into()))])).is_err());
    }

    #[test]
    fn supports_multiple_typed_changes_and_rejects_invalid_xml_text() {
        let xml = br#"<worksheet><sheetData><row><c r="A1"/><c r="B1" s="3"><v>1</v></c><c r="C1"><v>2</v></c></row></sheetData></worksheet>"#;
        let changes = BTreeMap::from([((0, 0), CellInput::Boolean(true)), ((0, 1), CellInput::Formula("A1<2".into())), ((0, 2), CellInput::Clear)]);
        let result = String::from_utf8(patch_existing_cells(xml, &changes).unwrap()).unwrap();
        assert!(result.contains("<c r=\"A1\" t=\"b\"><v>1</v></c>"));
        assert!(result.contains("<c r=\"B1\" s=\"3\"><f>A1&lt;2</f></c>"));
        assert!(result.contains("<c r=\"C1\"/>"));
        assert!(patch_existing_cells(xml, &BTreeMap::from([((0, 0), CellInput::Text("\0".into()))])).is_err());
        let prefixed = br#"<x:worksheet xmlns:x="urn:test"><x:sheetData><x:row><x:c r="A1"/></x:row></x:sheetData></x:worksheet>"#;
        assert!(patch_existing_cells(prefixed, &BTreeMap::from([((0, 0), CellInput::Number(3.0))])).is_err());
    }

    #[test]
    fn inserts_into_existing_rows_in_order_and_expands_dimension() {
        let xml = br#"<worksheet><dimension ref="B3:D3"/><sheetData><row r="3"><c r="B3"><v>2</v></c><c r="D3"><v>4</v></c></row></sheetData></worksheet>"#;
        let edits = BTreeMap::from([((2, 0), CellInput::Text("a".into())), ((2, 2), CellInput::Number(3.0)), ((2, 4), CellInput::Number(5.0))]);
        let result = String::from_utf8(patch_existing_cells(xml, &edits).unwrap()).unwrap();
        assert!(result.contains("<dimension ref=\"A3:E3\"/>"));
        let positions = ["A3", "B3", "C3", "D3", "E3"].map(|cell| result.find(&format!("r=\"{cell}\"")).unwrap());
        assert!(positions.windows(2).all(|p| p[0] < p[1]));
        let next = String::from_utf8(patch_existing_cells(xml, &BTreeMap::from([((3, 0), CellInput::Number(7.0))])).unwrap()).unwrap();
        assert!(next.contains("<row r=\"4\"><c r=\"A4\"><v>7</v></c></row>"));
    }

    #[test]
    fn inserts_rows_in_order_and_expands_empty_elements() {
        let xml = br#"<worksheet><dimension ref="B3:B5"/><sheetData><row r="3"><c r="B3"><v>1</v></c></row><row r="5" ht="22" customHeight="1"/></sheetData></worksheet>"#;
        let edits = BTreeMap::from([((0, 0), CellInput::Number(10.0)), ((3, 0), CellInput::Number(40.0)), ((4, 2), CellInput::Text("五".into())), ((5, 0), CellInput::Number(60.0))]);
        let result = String::from_utf8(patch_existing_cells(xml, &edits).unwrap()).unwrap();
        assert!(result.contains("<dimension ref=\"A1:C6\"/>"));
        let positions = [1, 3, 4, 5, 6].map(|r| result.find(&format!("<row r=\"{r}\"")).unwrap());
        assert!(positions.windows(2).all(|p| p[0] < p[1]));
        assert!(result.contains("<row r=\"5\" ht=\"22\" customHeight=\"1\"><c r=\"C5\""));
        let empty = br#"<worksheet><dimension ref="A1"/><sheetData/></worksheet>"#;
        let result = String::from_utf8(patch_existing_cells(empty, &BTreeMap::from([((9, 2), CellInput::Number(5.0))])).unwrap()).unwrap();
        assert!(result.contains("<dimension ref=\"A1:C10\"/>"));
        assert!(result.contains("<sheetData><row r=\"10\"><c r=\"C10\"><v>5</v></c></row></sheetData>"));
    }

    #[test]
    fn inserts_a_large_sparse_batch_without_changing_existing_cells() {
        let mut xml = String::from("<worksheet><dimension ref=\"A1:C4000\"/><sheetData>");
        let mut edits = BTreeMap::new();
        for row in 0..4_000u32 {
            let number = row + 1;
            xml.push_str(&format!("<row r=\"{number}\"><c r=\"A{number}\"><v>1</v></c><c r=\"C{number}\"><v>3</v></c></row>"));
            edits.insert((row, 1), CellInput::Number(2.0));
        }
        xml.push_str("</sheetData></worksheet>");
        let output = String::from_utf8(patch_existing_cells(xml.as_bytes(), &edits).unwrap()).unwrap();
        assert_eq!(output.matches("<v>1</v>").count(), 4_000);
        assert_eq!(output.matches("<v>2</v>").count(), 4_000);
        assert_eq!(output.matches("<v>3</v>").count(), 4_000);
        assert!(output.contains("<c r=\"B4000\"><v>2</v></c>"));
    }
}
