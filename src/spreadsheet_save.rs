//! Prepare an XLSX edit from its opening snapshot and commit only if the file
//! is still unchanged. The pending edit state remains with the caller on error.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::io::Write;
use std::sync::Arc;
use fdl_core::spreadsheet_edit::{Address, Arithmetic, CellInput, ReplaceMode, ReplaceOptions, Session};

#[derive(Debug)]
pub(crate) enum SaveError {
    Changed,
    Unsupported(anyhow::Error),
    Io(std::io::Error),
}

struct SavedWorkbook {
    snapshot: Vec<u8>,
    workbook: fdl_core::spreadsheet::Workbook,
    hidden: BTreeSet<Address>,
}

fn prepare(snapshot: &[u8], edits: &BTreeMap<Address, CellInput>, styles: &BTreeMap<Address, u32>) -> Result<Vec<u8>, SaveError> {
    let data = fdl_core::spreadsheet_xlsx::patch(snapshot, edits).map_err(SaveError::Unsupported)?;
    fdl_core::spreadsheet_xlsx::patch_style_history(&data, styles).map_err(SaveError::Unsupported)
}

fn save(path: &Path, snapshot: &[u8], edits: &BTreeMap<Address, CellInput>, styles: &BTreeMap<Address, u32>) -> Result<SavedWorkbook, SaveError> {
    let data = prepare(snapshot, edits, styles)?;
    publish(path, snapshot, data)
}

fn publish(path: &Path, snapshot: &[u8], data: Vec<u8>) -> Result<SavedWorkbook, SaveError> {
    // Check the generated archive before publishing it, not after replacing the source.
    let workbook = fdl_core::spreadsheet::read_xlsx_snapshot(&data).map_err(SaveError::Unsupported)?;
    let hidden = fdl_core::spreadsheet_xlsx::hidden_formulas(&data, &workbook);
    crate::workspace_fs::write_regular_if_unchanged(path, snapshot, &data).map_err(|error| match error {
        crate::workspace_fs::SaveFileError::Changed => SaveError::Changed,
        crate::workspace_fs::SaveFileError::Io(error) => SaveError::Io(error),
        _ => SaveError::Io(std::io::Error::other("unexpected workbook save failure")),
    })?;
    Ok(SavedWorkbook { snapshot: data, workbook, hidden })
}

pub(crate) struct SpreadsheetDocument {
    pub path: PathBuf,
    pub session: Session,
    snapshot: Arc<[u8]>,
}

impl SpreadsheetDocument {
    fn accept_saved(&mut self, saved: SavedWorkbook) {
        self.snapshot = saved.snapshot.into();
        self.session.mark_saved(saved.workbook);
        self.session.set_hidden_formulas(saved.hidden);
    }

    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let loaded = fdl_core::spreadsheet::load_xlsx(&path)?;
        let hidden = fdl_core::spreadsheet_xlsx::hidden_formulas(&loaded.snapshot, &loaded.workbook);
        let mut session = Session::new(loaded.workbook);
        session.set_hidden_formulas(hidden);
        Ok(Self { path, session, snapshot: loaded.snapshot.into() })
    }

    pub fn calculation_snapshot(&self) -> Arc<[u8]> { self.snapshot.clone() }

    /// On failure the edit overlay and its original snapshot remain intact.
    pub fn save(&mut self) -> Result<(), SaveError> {
        if !self.session.is_dirty() { return Ok(()); }
        let saved = save(&self.path, &self.snapshot, self.session.edits(), self.session.style_edits())?;
        self.accept_saved(saved);
        Ok(())
    }

    /// Preflight the entire replacement before applying the undoable edit.
    /// A later external-file conflict leaves that edit available for recovery.
    #[cfg(test)]
    fn replace_all(&mut self, sheet: usize, needle: &str, replacement: &str, mode: ReplaceMode) -> Result<usize, SaveError> {
        self.replace_all_with_options(sheet, needle, replacement, mode, &Default::default(), ReplaceOptions::default())
    }

    pub fn replace_all_with_options(&mut self, sheet: usize, needle: &str, replacement: &str, mode: ReplaceMode, styles: &fdl_core::spreadsheet_calc::PercentageStyles, options: ReplaceOptions) -> Result<usize, SaveError> {
        if self.session.is_dirty() { return Err(SaveError::Io(std::io::Error::other("save pending cell edits before replacing"))); }
        let edits = self.session.plan_replace_all_with_options(sheet, needle, replacement, mode, styles, options).map_err(SaveError::Unsupported)?;
        self.apply_batch(edits)
    }

    pub fn arithmetic_range(&mut self, a: Address, b: Address, operation: Arithmetic, operand: f64, include_formulas: bool, styles: &fdl_core::spreadsheet_calc::PercentageStyles) -> Result<usize, SaveError> {
        if self.session.is_dirty() { return Err(SaveError::Io(std::io::Error::other("save pending cell edits before arithmetic"))); }
        let edits = self.session.plan_arithmetic(a, b, operation, operand, include_formulas, styles).map_err(SaveError::Unsupported)?;
        self.apply_batch(edits)
    }

    pub fn format_number_range(&mut self, a: Address, b: Address, format: fdl_core::spreadsheet_xlsx::NumberFormat) -> Result<usize, SaveError> {
        if self.session.is_dirty() { return Err(SaveError::Io(std::io::Error::other("save pending cell edits before formatting"))); }
        let prepared = fdl_core::spreadsheet_xlsx::format_number_range(&self.snapshot, a, b, format).map_err(SaveError::Unsupported)?;
        let count = prepared.changes.len();
        if count == 0 { return Ok(0); }
        self.session.validate_saved_styles(&prepared.changes).map_err(SaveError::Unsupported)?;
        let saved = publish(&self.path, &self.snapshot, prepared.snapshot)?;
        self.snapshot = saved.snapshot.into();
        self.session.record_saved_styles(&prepared.changes, saved.workbook);
        self.session.set_hidden_formulas(saved.hidden);
        Ok(count)
    }

    fn apply_batch(&mut self, edits: BTreeMap<Address, CellInput>) -> Result<usize, SaveError> {
        let count = edits.len();
        if count == 0 { return Ok(0); }
        let data = fdl_core::spreadsheet_xlsx::patch(&self.snapshot, &edits).map_err(SaveError::Unsupported)?;
        self.session.set_many(edits).map_err(SaveError::Unsupported)?;
        let saved = publish(&self.path, &self.snapshot, data)?;
        self.accept_saved(saved);
        Ok(count)
    }

    /// Add a sheet only from a clean snapshot, retaining existing cell undo
    /// history. A conflicting external update is never intentionally replaced.
    pub fn add_worksheet(&mut self, name: &str) -> Result<(), SaveError> {
        if self.session.is_dirty() { return Err(SaveError::Io(std::io::Error::other("save pending cell edits before adding a worksheet"))); }
        let data = fdl_core::spreadsheet_xlsx::add_worksheet(&self.snapshot, name).map_err(SaveError::Unsupported)?;
        let saved = publish(&self.path, &self.snapshot, data)?;
        self.accept_saved(saved);
        Ok(())
    }

    pub fn rename_worksheet(&mut self, index: usize, name: &str) -> Result<(), SaveError> {
        if self.session.is_dirty() { return Err(SaveError::Io(std::io::Error::other("save pending cell edits before renaming a worksheet"))); }
        let old = self.session.workbook.sheets.get(index).ok_or_else(|| SaveError::Io(std::io::Error::other("worksheet does not exist")))?.name.clone();
        if old == name { return Ok(()); }
        if self.session.history_mentions_sheet(&old) { return Err(SaveError::Unsupported(anyhow::anyhow!("undo history contains a possible worksheet reference"))); }
        let data = fdl_core::spreadsheet_xlsx::rename_worksheet(&self.snapshot, index, name).map_err(SaveError::Unsupported)?;
        let saved = publish(&self.path, &self.snapshot, data)?;
        self.accept_saved(saved);
        Ok(())
    }

    /// This structural deletion cannot be undone by the cell undo stack.
    /// Callers must clearly confirm both deletion and history reset.
    pub fn delete_worksheet(&mut self, index: usize) -> Result<(), SaveError> {
        if self.session.is_dirty() { return Err(SaveError::Io(std::io::Error::other("save pending cell edits before deleting a worksheet"))); }
        let data = fdl_core::spreadsheet_xlsx::delete_worksheet(&self.snapshot, index).map_err(SaveError::Unsupported)?;
        let saved = publish(&self.path, &self.snapshot, data)?;
        self.accept_saved(saved);
        self.session.clear_history();
        Ok(())
    }

    /// Publish a new workbook without replacing any existing path. On success
    /// the document follows the new copy; on failure all pending edits remain.
    pub fn save_copy(&mut self, destination: &Path) -> Result<(), SaveError> {
        let same_type = self.path.extension().and_then(|v| v.to_str()).zip(destination.extension().and_then(|v| v.to_str()))
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b));
        if !same_type { return Err(SaveError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, "copy must retain the original workbook extension"))); }
        let data = prepare(&self.snapshot, self.session.edits(), self.session.style_edits())?;
        let workbook = fdl_core::spreadsheet::read_xlsx_snapshot(&data).map_err(SaveError::Unsupported)?;
        let hidden = fdl_core::spreadsheet_xlsx::hidden_formulas(&data, &workbook);
        let parent = destination.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
        let staging = parent.join(format!(".markdownx-copy-{}.tmp", uuid::Uuid::new_v4()));
        let mut created = false;
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&staging)?;
            created = true;
            file.write_all(&data)?;
            file.sync_all()?;
            drop(file);
            renamore::rename_exclusive(&staging, destination)
        })();
        if result.is_err() && created { let _ = std::fs::remove_file(&staging); }
        result.map_err(SaveError::Io)?;
        self.path = destination.to_path_buf();
        self.snapshot = data.into();
        self.session.mark_saved(workbook);
        self.session.set_hidden_formulas(hidden);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn protected_hidden_formulas_stay_out_of_editor_search_and_clipboard() {
        use fdl_core::spreadsheet_calc::FormulaResult;
        let root = std::env::temp_dir().join(format!("markdownx-hidden-formula-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("book.xlsx");
        let source = include_bytes!("../crates/core/tests/fixtures/hidden_formulas.xlsx");
        fs::write(&path, source).unwrap();
        let mut document = SpreadsheetDocument::open(path.clone()).unwrap();
        let a = |sheet, column| Address { sheet, row: 0, column };
        assert!(document.session.is_hidden_formula(a(0, 1)));
        assert!(document.session.is_hidden_formula(a(0, 3)));
        assert!(!document.session.is_hidden_formula(a(0, 2)));
        assert!(!document.session.is_hidden_formula(a(1, 0)));
        assert_eq!(document.session.display_input(a(0, 1)), Some(CellInput::Clear));
        assert_eq!(document.session.display_input(a(0, 2)), Some(CellInput::Formula("A1+3".into())));
        assert!(document.session.copy_cells(a(0, 0), a(0, 1)).is_err());
        assert!(document.session.set(a(0, 3), CellInput::Number(5.0)).is_err());
        let results = BTreeMap::from([(a(0, 1), FormulaResult::Number(20.0))]);
        assert_eq!(document.session.copy_values_tsv(a(0, 1), a(0, 1), &results).unwrap(), "20");
        assert_eq!(document.session.find_cell(0, "A1*2", None, false, &results, &BTreeMap::new()), None);
        assert_eq!(document.session.find_cell(0, "20", None, false, &results, &BTreeMap::new()), Some(a(0, 1)));
        assert!(!document.session.plan_replace_all(0, "A1", "B1", ReplaceMode::Formula).unwrap().contains_key(&a(0, 1)));
        assert_eq!(fs::read(&path).unwrap(), source);
        let new_formula = a(0, 4);
        document.session.set(new_formula, CellInput::Formula("A1*3".into())).unwrap();
        document.save().unwrap();
        assert!(document.session.is_hidden_formula(new_formula));
        assert_eq!(document.session.display_input(new_formula), Some(CellInput::Clear));
        assert!(document.session.undo());
        document.save().unwrap();
        assert!(!document.session.is_hidden_formula(new_formula));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn number_formats_are_saved_undoable_and_do_not_overwrite_conflicts() {
        use fdl_core::spreadsheet_xlsx::NumberFormat;
        let root = std::env::temp_dir().join(format!("markdownx-format-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("book.xlsx");
        let a = Address { sheet: 0, row: 0, column: 0 };
        let b = Address { sheet: 0, row: 0, column: 1 };
        let source = fdl_core::spreadsheet_xlsx::patch(include_bytes!("../crates/core/tests/fixtures/number_formats.xlsx"), &BTreeMap::from([(a, CellInput::Number(0.126))])).unwrap();
        fs::write(&path, source).unwrap();
        let mut document = SpreadsheetDocument::open(path.clone()).unwrap();
        let display = |path: &Path, address| fdl_core::spreadsheet_calc::evaluate(&fs::read(path).unwrap()).unwrap().formatted[&address].clone();
        assert_eq!(display(&path, a), "12.60%");
        assert_eq!(document.format_number_range(a, b, NumberFormat::Decimal2).unwrap(), 2);
        assert_eq!(display(&path, a), "0.13");
        assert_eq!(display(&path, b), "0.25");
        assert!(document.session.undo());
        document.save().unwrap();
        assert_eq!(display(&path, a), "12.60%");
        assert_eq!(display(&path, b), "25.2%");
        assert!(document.session.redo());
        document.save().unwrap();
        assert_eq!(display(&path, a), "0.13");
        document.session.set(a, CellInput::Number(0.5)).unwrap();
        document.save().unwrap();
        assert!(document.session.undo());
        document.save().unwrap();
        assert!(document.session.undo());
        document.save().unwrap();
        assert_eq!(display(&path, a), "12.60%");
        assert!(document.session.redo());
        document.save().unwrap();
        assert_eq!(display(&path, a), "0.13");
        fs::write(&path, b"external change").unwrap();
        assert!(matches!(document.format_number_range(a, b, NumberFormat::Percent2), Err(SaveError::Changed)));
        assert_eq!(fs::read(&path).unwrap(), b"external change");
        assert!(!document.session.is_dirty());
        assert!(document.session.undo());
        assert!(matches!(document.save(), Err(SaveError::Changed)));
        assert!(document.session.is_dirty());
        let recovered = root.join("recovered.xlsx");
        document.save_copy(&recovered).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"external change");
        assert_eq!(display(&recovered, a), "12.60%");
        assert!(!document.session.is_dirty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn protected_sheet_keeps_locked_edits_pending_and_allows_unlocked_cells() {
        let root = std::env::temp_dir().join(format!("markdownx-sheet-protection-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("book.xlsx");
        fs::write(&path, include_bytes!("../crates/core/tests/fixtures/sheet_protection.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(path.clone()).unwrap();
        let locked = Address { sheet: 0, row: 0, column: 0 };
        let unlocked = Address { sheet: 0, row: 0, column: 1 };
        let original = fs::read(&path).unwrap();
        assert!(matches!(document.replace_all(0, "e", "X", ReplaceMode::Text), Err(SaveError::Unsupported(_))));
        assert!(!document.session.is_dirty());
        assert_eq!(fs::read(&path).unwrap(), original);
        document.session.set(locked, CellInput::Text("pending".into())).unwrap();
        assert!(matches!(document.save(), Err(SaveError::Unsupported(_))));
        assert!(document.session.is_dirty());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(document.session.undo());
        document.session.set(unlocked, CellInput::Text("saved".into())).unwrap();
        document.save().unwrap();
        let book = fdl_core::spreadsheet::read(&path).unwrap();
        assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 0 && cell.value == "locked"));
        assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 1 && cell.value == "saved"));
        assert!(document.session.undo());
        document.save().unwrap();
        assert_eq!(document.session.input(unlocked), Some(CellInput::Text("editable".into())));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replace_all_is_undoable_preflighted_and_preserves_conflicts() {
        let root = std::env::temp_dir().join(format!("markdownx-replace-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("book.xlsx");
        fs::write(&path, include_bytes!("../assets/empty-workbook.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(path.clone()).unwrap();
        let a = Address { sheet: 0, row: 0, column: 0 };
        let b = Address { sheet: 0, row: 0, column: 1 };
        document.session.set_many([(a, CellInput::Text("生命 HP".into())), (b, CellInput::Formula("A1&\"HP\"".into()))]).unwrap();
        document.save().unwrap();
        assert_eq!(document.replace_all(0, "HP", "MP", ReplaceMode::Text).unwrap(), 1);
        assert_eq!(document.session.input(a), Some(CellInput::Text("生命 MP".into())));
        assert_eq!(document.session.input(b), Some(CellInput::Formula("A1&\"HP\"".into())));
        assert_eq!(document.replace_all(0, "HP", "MP", ReplaceMode::Formula).unwrap(), 1);
        assert_eq!(document.session.input(b), Some(CellInput::Formula("A1&\"MP\"".into())));
        assert!(document.session.undo());
        document.save().unwrap();
        assert_eq!(document.session.input(b), Some(CellInput::Formula("A1&\"HP\"".into())));
        fs::write(&path, b"external change").unwrap();
        assert!(matches!(document.replace_all(0, "MP", "SP", ReplaceMode::Text), Err(SaveError::Changed)));
        assert_eq!(fs::read(&path).unwrap(), b"external change");
        assert!(document.session.is_dirty());
        assert_eq!(document.session.input(a), Some(CellInput::Text("生命 SP".into())));
        assert!(document.session.undo());
        assert_eq!(document.session.input(a), Some(CellInput::Text("生命 MP".into())));
        let protected = root.join("protected.xlsx");
        fs::write(&protected, include_bytes!("../crates/core/tests/fixtures/protected_ranges.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(protected.clone()).unwrap();
        let original = fs::read(&protected).unwrap();
        assert!(matches!(document.replace_all(0, "A1", "B1", ReplaceMode::Formula), Err(SaveError::Unsupported(_))));
        assert!(!document.session.is_dirty());
        assert_eq!(fs::read(&protected).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replace_values_saves_preserves_formats_and_rejects_invalid_batches() {
        let root = std::env::temp_dir().join(format!("markdownx-replace-values-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("book.xlsx");
        fs::write(&path, include_bytes!("../crates/core/tests/fixtures/replacements.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(path.clone()).unwrap();
        let styles = fdl_core::spreadsheet_calc::evaluate(&document.calculation_snapshot()).unwrap().percentage_styles;
        let options = ReplaceOptions { case_sensitive: false, whole_cell: true };
        let original = fs::read(&path).unwrap();
        assert!(matches!(document.replace_all_with_options(0, "10", "invalid", ReplaceMode::Values, &styles, options), Err(SaveError::Unsupported(_))));
        assert!(!document.session.is_dirty());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(document.replace_all_with_options(0, "0.125", "0.5", ReplaceMode::Values, &styles, options).unwrap(), 1);
        assert_eq!(document.replace_all_with_options(0, "2025-01-02", "2026-02-03", ReplaceMode::Values, &styles, options).unwrap(), 1);
        assert_eq!(document.replace_all_with_options(0, "true", "FALSE", ReplaceMode::Values, &styles, options).unwrap(), 1);
        let evaluation = fdl_core::spreadsheet_calc::evaluate(&fs::read(&path).unwrap()).unwrap();
        let address = |column| Address { sheet: 0, row: 0, column };
        assert_eq!(evaluation.formatted[&address(3)], "50.00%");
        assert_eq!(evaluation.formatted[&address(4)], "2026-02-03");
        assert_eq!(document.session.input(address(5)), Some(CellInput::Boolean(false)));
        assert!(document.session.undo());
        document.save().unwrap();
        assert_eq!(document.session.input(address(5)), Some(CellInput::Boolean(true)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn arithmetic_batch_preserves_styles_undo_and_external_changes() {
        let root = std::env::temp_dir().join(format!("markdownx-arithmetic-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("book.xlsx");
        fs::write(&path, include_bytes!("../crates/core/tests/fixtures/number_formats.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(path.clone()).unwrap();
        let styles = fdl_core::spreadsheet_calc::evaluate(&document.calculation_snapshot()).unwrap().percentage_styles;
        let a = Address { sheet: 0, row: 0, column: 0 };
        let b = Address { sheet: 0, row: 3, column: 1 };
        let original = fs::read(&path).unwrap();
        assert!(matches!(document.arithmetic_range(a, b, Arithmetic::Divide, 0.0, true, &styles), Err(SaveError::Unsupported(_))));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!document.session.is_dirty());
        assert_eq!(document.arithmetic_range(a, b, Arithmetic::Multiply, 2.0, true, &styles).unwrap(), 4);
        let evaluation = fdl_core::spreadsheet_calc::evaluate(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(evaluation.formatted[&a], "25.00%");
        assert_eq!(evaluation.formatted[&Address { column: 1, ..a }], "100.0%");
        assert_eq!(evaluation.formatted[&Address { row: 3, ..a }], "2025-01-02");
        assert!(document.session.undo());
        document.save().unwrap();
        assert_eq!(document.session.input(a), Some(CellInput::Number(0.125)));
        assert!(document.session.redo());
        document.save().unwrap();
        fs::write(&path, b"external change").unwrap();
        assert!(matches!(document.arithmetic_range(a, a, Arithmetic::Add, 1.0, false, &styles), Err(SaveError::Changed)));
        assert_eq!(fs::read(&path).unwrap(), b"external change");
        assert!(document.session.is_dirty());
        assert_eq!(document.session.input(a), Some(CellInput::Number(1.25)));
        assert!(document.session.undo());
        assert_eq!(document.session.input(a), Some(CellInput::Number(0.25)));
        let protected = root.join("protected.xlsx");
        fs::write(&protected, include_bytes!("../crates/core/tests/fixtures/protected_ranges.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(protected.clone()).unwrap();
        let styles = fdl_core::spreadsheet_calc::evaluate(&document.calculation_snapshot()).unwrap().percentage_styles;
        let array = Address { sheet: 0, row: 0, column: 3 };
        let original = fs::read(&protected).unwrap();
        assert!(matches!(document.arithmetic_range(array, array, Arithmetic::Multiply, 2.0, true, &styles), Err(SaveError::Unsupported(_))));
        assert!(!document.session.is_dirty());
        assert_eq!(fs::read(&protected).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adds_worksheet_without_overwriting_external_changes_or_pending_edits() {
        let root = std::env::temp_dir().join(format!("markdownx-sheet-add-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("book.xlsx");
        fs::write(&path, include_bytes!("../crates/core/tests/fixtures/spreadsheet.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(path.clone()).unwrap();
        let address = Address { sheet: 0, row: 3, column: 3 };
        document.session.set(address, CellInput::Number(42.0)).unwrap();
        assert!(document.add_worksheet("数值策划").is_err());
        assert!(document.session.is_dirty());
        document.save().unwrap();
        document.add_worksheet("数值策划").unwrap();
        assert_eq!(document.session.workbook.sheets.last().unwrap().name, "数值策划");
        document.rename_worksheet(2, "平衡").unwrap();
        assert_eq!(document.session.workbook.sheets.last().unwrap().name, "平衡");
        assert!(document.session.undo());
        document.save().unwrap();
        assert_eq!(fdl_core::spreadsheet::read(&path).unwrap().sheets.len(), 3);
        fs::write(&path, b"external change").unwrap();
        assert!(matches!(document.add_worksheet("冲突"), Err(SaveError::Changed)));
        assert_eq!(fs::read(&path).unwrap(), b"external change");
        assert_eq!(document.session.workbook.sheets.len(), 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keeps_undo_history_safe_during_sheet_rename() {
        let root = std::env::temp_dir().join(format!("markdownx-sheet-history-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("book.xlsx");
        fs::write(&path, include_bytes!("../assets/empty-workbook.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(path.clone()).unwrap();
        let address = Address { sheet: 0, row: 1, column: 1 };
        document.session.set(address, CellInput::Formula("Sheet1!A1".into())).unwrap();
        document.save().unwrap();
        assert!(document.session.undo());
        document.save().unwrap();
        assert!(document.session.can_redo());
        let before = fs::read(&path).unwrap();
        assert!(matches!(document.rename_worksheet(0, "新名称"), Err(SaveError::Unsupported(_))));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(document.session.can_redo());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delete_sheet_keeps_conflicting_files_and_clears_history_only_after_success() {
        let root = std::env::temp_dir().join(format!("markdownx-sheet-delete-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("book.xlsx");
        fs::write(&path, include_bytes!("../assets/empty-workbook.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(path.clone()).unwrap();
        document.add_worksheet("待删除").unwrap();
        document.session.set(Address { sheet: 1, row: 0, column: 0 }, CellInput::Number(7.0)).unwrap();
        document.save().unwrap();
        assert!(document.session.can_undo());
        let original = fs::read(&path).unwrap();
        fs::write(&path, b"external change").unwrap();
        assert!(matches!(document.delete_worksheet(1), Err(SaveError::Changed)));
        assert!(document.session.can_undo());
        assert_eq!(fs::read(&path).unwrap(), b"external change");
        fs::write(&path, original).unwrap();
        document.delete_worksheet(1).unwrap();
        assert_eq!(document.session.workbook.sheets.len(), 1);
        assert!(!document.session.can_undo() && !document.session.can_redo());
        assert_eq!(fdl_core::spreadsheet::read(&path).unwrap().sheets.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn saves_and_rejects_stale_snapshots_without_losing_external_changes() {
        let root = std::env::temp_dir().join(format!("markdownx-spreadsheet-save-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("fixture.xlsx");
        let source = include_bytes!("../crates/core/tests/fixtures/spreadsheet.xlsx");
        fs::write(&path, source).unwrap();
        let edits = BTreeMap::from([(Address { sheet: 0, row: 2, column: 3 }, CellInput::Number(123.0))]);
        let saved = save(&path, source, &edits, &BTreeMap::new()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), saved.snapshot);
        assert!(fdl_core::spreadsheet::read(&path).unwrap().sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == 123.0));
        let external = b"external change";
        fs::write(&path, external).unwrap();
        assert!(matches!(save(&path, &saved.snapshot, &edits, &BTreeMap::new()), Err(SaveError::Changed)));
        assert_eq!(fs::read(&path).unwrap(), external);
        fs::write(&path, source).unwrap();
        let invalid = BTreeMap::from([(Address { sheet: 42, row: 0, column: 0 }, CellInput::Number(1.0))]);
        assert!(matches!(save(&path, source, &invalid, &BTreeMap::new()), Err(SaveError::Unsupported(_))));
        assert_eq!(fs::read(&path).unwrap(), source);
        let permissions = fs::metadata(&path).unwrap().permissions();
        let mut read_only = permissions.clone();
        read_only.set_readonly(true);
        fs::set_permissions(&path, read_only).unwrap();
        assert!(matches!(save(&path, source, &edits, &BTreeMap::new()), Err(SaveError::Io(_))));
        assert_eq!(fs::read(&path).unwrap(), source);
        fs::set_permissions(&path, permissions).unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert!(root.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn document_keeps_edits_after_conflict_and_resets_after_save() {
        let root = std::env::temp_dir().join(format!("markdownx-spreadsheet-document-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("fixture.xlsx");
        fs::write(&path, include_bytes!("../crates/core/tests/fixtures/spreadsheet.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(path.clone()).unwrap();
        let cell = Address { sheet: 0, row: 2, column: 3 };
        document.session.set(cell, CellInput::Text("数值".into())).unwrap();
        let external = b"external change";
        fs::write(&path, external).unwrap();
        assert!(matches!(document.save(), Err(SaveError::Changed)));
        assert!(document.session.is_dirty());
        assert_eq!(fs::read(&path).unwrap(), external);
        fs::write(&path, include_bytes!("../crates/core/tests/fixtures/spreadsheet.xlsx")).unwrap();
        document.save().unwrap();
        assert!(!document.session.is_dirty());
        assert!(document.session.workbook.sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == "数值"));
        assert!(document.session.undo());
        document.save().unwrap();
        assert!(document.session.workbook.sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == "中文"));
        assert!(document.session.redo());
        document.save().unwrap();
        assert!(document.session.workbook.sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == "数值"));
        assert!(root.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_copy_recovers_conflict_without_overwriting_any_file() {
        let root = std::env::temp_dir().join(format!("markdownx-spreadsheet-copy-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source.xlsx");
        let copy = root.join("copy.xlsx");
        fs::write(&source, include_bytes!("../crates/core/tests/fixtures/spreadsheet.xlsx")).unwrap();
        let mut document = SpreadsheetDocument::open(source.clone()).unwrap();
        document.session.set(Address { sheet: 0, row: 2, column: 3 }, CellInput::Text("恢复".into())).unwrap();
        fs::write(&source, b"external change").unwrap();
        fs::write(&copy, b"existing copy").unwrap();
        assert!(matches!(document.save_copy(&copy), Err(SaveError::Io(_))));
        assert_eq!(fs::read(&copy).unwrap(), b"existing copy");
        assert!(document.session.is_dirty());
        assert!(matches!(document.save_copy(&root.join("wrong.xlsm")), Err(SaveError::Io(_))));
        let recovered = root.join("recovered.xlsx");
        document.save_copy(&recovered).unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"external change");
        assert!(!document.session.is_dirty());
        assert_eq!(document.path, recovered);
        assert!(document.session.workbook.sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == "恢复"));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
        assert!(root.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(root).unwrap();
    }
}
