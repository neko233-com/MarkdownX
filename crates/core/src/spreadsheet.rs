//! Read spreadsheet values and formulas without modifying the source workbook.
//!
//! This is a loading foundation, not a lossless workbook editor: styles,
//! charts and other package parts are intentionally not written back.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::io::{Cursor, Read};
use anyhow::{Context, Result, ensure};
use calamine::{Data, Range, Reader, Xlsx, open_workbook_auto};

const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CELLS: usize = 1_000_000;
const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// Zero-based coordinates in the original worksheet.
    pub row: u32,
    pub column: u32,
    pub value: Data,
    /// Formula as provided by the workbook, without recalculation.
    pub formula: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
}

/// The exact archive bytes that produced the displayed workbook. Keeping both
/// together lets the save path detect external changes against the right data.
pub struct LoadedWorkbook {
    pub workbook: Workbook,
    pub snapshot: Vec<u8>,
}

/// Load cached values and formulas. This function never saves or changes a file.
pub fn read(path: &Path) -> Result<Workbook> {
    let meta = std::fs::metadata(path).context("cannot inspect workbook")?;
    ensure!(meta.is_file(), "workbook is not a regular file");
    ensure!(meta.len() <= MAX_FILE_BYTES, "workbook exceeds the file size limit");
    if path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["xlsx", "xlsm", "xlam"].iter().any(|x| e.eq_ignore_ascii_case(x))) {
        return Ok(load_xlsx(path)?.workbook);
    }
    let mut book = open_workbook_auto(path).context("cannot open workbook")?;
    let names = book.sheet_names().to_vec();
    let mut sheets = Vec::with_capacity(names.len());
    let mut remaining = MAX_CELLS;
    for name in names {
        let values = book.worksheet_range(&name).with_context(|| format!("cannot read worksheet {name:?}"))?;
        let formulas = book.worksheet_formula(&name).with_context(|| format!("cannot read formulas in worksheet {name:?}"))?;
        let sheet = collect_sheet(name, &values, &formulas, remaining)?;
        remaining -= sheet.cells.len();
        sheets.push(sheet);
    }
    Ok(Workbook { sheets })
}

/// XLSX worksheets are streamed: a cell at the far edge does not allocate the
/// entire rectangular area between it and another cell.
pub fn load_xlsx(path: &Path) -> Result<LoadedWorkbook> {
    let file = std::fs::File::open(path).context("cannot open XLSX workbook")?;
    ensure!(file.metadata()?.is_file(), "workbook is not a regular file");
    let mut snapshot = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut snapshot)?;
    ensure!(snapshot.len() as u64 <= MAX_FILE_BYTES, "workbook exceeds the file size limit");
    let workbook = read_xlsx_snapshot(&snapshot)?;
    Ok(LoadedWorkbook { workbook, snapshot })
}

pub fn read_xlsx_snapshot(snapshot: &[u8]) -> Result<Workbook> {
    Ok(read_xlsx_snapshot_with_epoch(snapshot)?.0)
}

pub(crate) fn read_xlsx_snapshot_with_epoch(snapshot: &[u8]) -> Result<(Workbook, bool)> {
    ensure!(snapshot.len() as u64 <= MAX_FILE_BYTES, "workbook exceeds the file size limit");
    validate_xlsx_archive(snapshot)?;
    let mut book = Xlsx::new(Cursor::new(snapshot)).context("cannot open XLSX workbook")?;
    let is_1904 = book.has_1904_epoch();
    let names = book.sheet_names().to_vec();
    let mut sheets = Vec::with_capacity(names.len());
    let mut remaining = MAX_CELLS;
    for name in names {
        let mut reader = book.worksheet_cells_reader(&name).with_context(|| format!("cannot read worksheet {name:?}"))?;
        let mut cells = Vec::new();
        while let Some(source) = reader.next_cell_with_formula().with_context(|| format!("cannot read cells in worksheet {name:?}"))? {
            let value = Data::from(source.value);
            if value == Data::Empty && source.formula.is_none() { continue; }
            ensure!(cells.len() < remaining, "workbook exceeds the cell limit");
            cells.push(Cell { row: source.pos.0, column: source.pos.1, value, formula: source.formula });
        }
        remaining -= cells.len();
        sheets.push(Sheet { name, cells });
    }
    Ok((Workbook { sheets }, is_1904))
}

// Inspect the ZIP directory before handing the archive to parsers which may
// eagerly expand shared strings or styles. Signed workbooks remain readable.
fn validate_xlsx_archive(snapshot: &[u8]) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(snapshot)).context("invalid XLSX archive")?;
    ensure!(archive.len() <= MAX_ARCHIVE_ENTRIES, "too many archive entries");
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        ensure!(names.insert(entry.name().to_owned()), "duplicate archive entry");
        ensure!(entry.size() <= MAX_ENTRY_BYTES, "archive entry is too large");
        total = total.checked_add(entry.size()).context("archive size overflow")?;
        ensure!(total <= MAX_TOTAL_BYTES, "workbook is too large");
    }
    Ok(())
}

fn collect_sheet(name: String, values: &Range<Data>, formulas: &Range<String>, limit: usize) -> Result<Sheet> {
    let mut cells = BTreeMap::<(u32, u32), Cell>::new();
    if let Some((start_row, start_col)) = values.start() {
        for (row, column, value) in values.used_cells() {
            let key = (start_row + row as u32, start_col + column as u32);
            cells.insert(key, Cell { row: key.0, column: key.1, value: value.clone(), formula: None });
            ensure!(cells.len() <= limit, "workbook exceeds the cell limit");
        }
    }
    if let Some((start_row, start_col)) = formulas.start() {
        for (row, column, formula) in formulas.used_cells() {
            let key = (start_row + row as u32, start_col + column as u32);
            let cell = cells.entry(key).or_insert_with(|| Cell { row: key.0, column: key.1, value: Data::Empty, formula: None });
            cell.formula = Some(formula.clone());
            ensure!(cells.len() <= limit, "workbook exceeds the cell limit");
        }
    }
    Ok(Sheet { name, cells: cells.into_values().collect() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::Cell as SourceCell;

    #[test]
    fn rejects_oversized_archive_entries_before_parsing() {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file("xl/sharedStrings.xml", zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(b"small").unwrap();
        let mut data = writer.finish().unwrap().into_inner();
        assert!(validate_xlsx_archive(&data).is_ok());
        let central = data.windows(4).position(|s| s == b"PK\x01\x02").unwrap();
        data[central + 24..central + 28].copy_from_slice(&((MAX_ENTRY_BYTES + 1) as u32).to_le_bytes());
        assert!(validate_xlsx_archive(&data).is_err());
        assert!(validate_xlsx_archive(b"not a zip").is_err());
        assert!(validate_xlsx_archive(include_bytes!("../tests/fixtures/spreadsheet.xlsx")).is_ok());
    }

    #[test]
    fn preserves_absolute_coordinates_values_and_formulas() {
        let values = Range::from_sparse(vec![SourceCell::new((2, 3), Data::String("中文".into())), SourceCell::new((4, 5), Data::Float(7.0))]);
        let formulas = Range::from_sparse(vec![SourceCell::new((4, 5), "SUM(A1:A2)".to_string()), SourceCell::new((7, 1), "A1".to_string())]);
        let sheet = collect_sheet("数据".into(), &values, &formulas, 3).unwrap();
        assert_eq!(sheet.cells.len(), 3);
        assert_eq!((sheet.cells[0].row, sheet.cells[0].column), (2, 3));
        assert_eq!(sheet.cells[1].formula.as_deref(), Some("SUM(A1:A2)"));
        assert_eq!((sheet.cells[2].row, sheet.cells[2].column, &sheet.cells[2].value), (7, 1, &Data::Empty));
        assert!(collect_sheet("data".into(), &values, &formulas, 2).is_err());
    }

    #[test]
    fn reads_real_xlsx_without_modifying_it() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spreadsheet.xlsx");
        let before = std::fs::read(&path).unwrap();
        let loaded = load_xlsx(&path).unwrap();
        assert_eq!(loaded.snapshot, before);
        let book = loaded.workbook;
        assert_eq!(read_xlsx_snapshot(&before).unwrap(), book);
        assert_eq!(book.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["数据", "Second"]);
        assert!(book.sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == Data::String("中文".into())));
        assert!(book.sheets[0].cells.iter().any(|c| c.row == 4 && c.column == 5 && c.formula.as_deref() == Some("SUM(1,2)")));
        assert!(book.sheets[1].cells.iter().any(|c| c.row == 1 && c.column == 1 && c.value == Data::Bool(true)));
        assert!(book.sheets[1].cells.iter().any(|c| c.row == 1_048_575 && c.column == 16_383 && c.value == Data::String("far".into())));
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}
