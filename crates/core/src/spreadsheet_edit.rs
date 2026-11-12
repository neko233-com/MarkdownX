//! Pending spreadsheet cell edits. The source workbook stays untouched until
//! a separate, validated save operation commits these changes.
use std::collections::{BTreeMap, BTreeSet};
use anyhow::{Result, ensure};
use crate::spreadsheet::Workbook;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CellInput {
    Clear,
    Text(String),
    Number(f64),
    Boolean(bool),
    /// Formula text without the leading equals sign.
    Formula(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplaceMode { Text, Formula, Values }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arithmetic { Add, Subtract, Multiply, Divide }

impl Arithmetic {
    fn symbol(self) -> &'static str {
        match self { Self::Add => "+", Self::Subtract => "-", Self::Multiply => "*", Self::Divide => "/" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplaceOptions { pub case_sensitive: bool, pub whole_cell: bool }

impl Default for ReplaceOptions {
    fn default() -> Self { Self { case_sensitive: true, whole_cell: false } }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CellClipboard {
    pub sheet: usize,
    pub row: u32,
    pub column: u32,
    pub width: u32,
    pub height: u32,
    pub cells: Vec<CellInput>,
}

impl CellClipboard {
    /// Plain-text fallback for other applications; internal paste uses the
    /// typed metadata to preserve identifiers, booleans and formulas.
    pub fn plain_tsv(&self) -> Result<String> {
        ensure!(self.width > 0 && self.height > 0 && u64::from(self.width) * u64::from(self.height) == self.cells.len() as u64 && self.cells.len() <= 100_000, "invalid clipboard dimensions");
        let mut output = String::new();
        for (i, cell) in self.cells.iter().enumerate() {
            if i > 0 { output.push(if (i as u32).is_multiple_of(self.width) { '\n' } else { '\t' }); }
            let value = match cell {
                CellInput::Clear => String::new(),
                CellInput::Text(s) if parse_input(s) != *cell || match parse_input_with_format(s, Some(true)) { Ok(value) => value != *cell, Err(_) => true } => format!("'{s}"),
                CellInput::Text(s) => s.clone(),
                CellInput::Number(n) => n.to_string(),
                CellInput::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                CellInput::Formula(s) => format!("={s}"),
            };
            ensure!(output.len().saturating_add(value.len().saturating_mul(2)).saturating_add(2) <= 16 * 1024 * 1024, "copied selection is too large");
            if value.contains(['\t', '\n', '\r', '"']) {
                output.push('"');
                for c in value.chars() { if c == '"' { output.push('"'); } output.push(c); }
                output.push('"');
            } else { output.push_str(&value); }
        }
        Ok(output)
    }
}

fn parse_copy_formula(parser: &mut ironcalc::base::expressions::parser::Parser<'_>, formula: &str, source: &ironcalc::base::expressions::types::CellReferenceRC) -> Result<ironcalc::base::expressions::parser::Node> {
    use ironcalc::base::expressions::parser::{Node, stringify::to_excel_string};
    let mut in_string = false;
    for c in formula.chars() {
        if c == '"' { in_string = !in_string; }
        else if !in_string && (c == '[' || c == ']') { anyhow::bail!("structured and external references cannot be translated safely"); }
    }
    let ast = parser.parse(formula, source);
    ensure!(!matches!(ast, Node::ParseErrorKind { .. }) && to_excel_string(&ast, source) == formula, "formula cannot be translated safely");
    Ok(ast)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Address {
    pub sheet: usize,
    pub row: u32,
    pub column: u32,
}

#[derive(Clone, Debug)]
enum Change {
    Value { address: Address, before: CellInput, after: CellInput },
    Style { address: Address, before: u32, after: u32 },
}

/// Editing state is a sparse overlay so blank cells do not require allocating
/// a rectangular grid. Cached formula values are not recalculated here.
pub struct Session {
    pub workbook: Workbook,
    originals: BTreeMap<Address, Option<CellInput>>,
    dates: BTreeMap<Address, bool>,
    edits: BTreeMap<Address, CellInput>,
    style_originals: BTreeMap<Address, u32>,
    style_edits: BTreeMap<Address, u32>,
    hidden_formulas: BTreeSet<Address>,
    undo: Vec<Vec<Change>>,
    redo: Vec<Vec<Change>>,
}

impl Session {
    pub fn new(workbook: Workbook) -> Self {
        let mut originals = BTreeMap::new();
        let mut dates = BTreeMap::new();
        for (sheet, data) in workbook.sheets.iter().enumerate() {
            for cell in &data.cells {
                let address = Address { sheet, row: cell.row, column: cell.column };
                originals.insert(address, original_cell(cell));
                if cell.formula.is_none() && let calamine::Data::DateTime(date) = &cell.value && date.is_datetime() && (0.0..2_958_466.0).contains(&date.as_f64()) {
                    let alternate = calamine::ExcelDateTime::new(date.as_f64(), calamine::ExcelDateTimeType::DateTime, true);
                    dates.insert(address, date.to_ymd_hms_milli() == alternate.to_ymd_hms_milli());
                }
            }
        }
        Self { workbook, originals, dates, edits: BTreeMap::new(), style_originals: BTreeMap::new(), style_edits: BTreeMap::new(), hidden_formulas: BTreeSet::new(), undo: Vec::new(), redo: Vec::new() }
    }

    pub fn edits(&self) -> &BTreeMap<Address, CellInput> { &self.edits }
    pub fn style_edits(&self) -> &BTreeMap<Address, u32> { &self.style_edits }
    pub fn is_dirty(&self) -> bool { !self.edits.is_empty() || !self.style_edits.is_empty() }
    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
    pub fn clear_history(&mut self) { self.undo.clear(); self.redo.clear(); self.style_originals.clear(); self.style_edits.clear(); }
    pub fn set_hidden_formulas(&mut self, hidden: BTreeSet<Address>) { self.hidden_formulas = hidden; }
    pub fn is_hidden_formula(&self, address: Address) -> bool { self.hidden_formulas.contains(&address) }

    /// Plan a case-sensitive literal replacement in one worksheet.
    pub fn plan_replace_all(&self, sheet: usize, needle: &str, replacement: &str, mode: ReplaceMode) -> Result<BTreeMap<Address, CellInput>> {
        self.plan_replace_all_with_options(sheet, needle, replacement, mode, &Default::default(), ReplaceOptions::default())
    }

    /// Text, formula source, and editable values are deliberately separate.
    /// Numeric values use their unformatted source, while existing date cells
    /// use an ISO date so edits retain their workbook's epoch and number format.
    pub fn plan_replace_all_with_options(&self, sheet: usize, needle: &str, replacement: &str, mode: ReplaceMode, styles: &crate::spreadsheet_calc::PercentageStyles, options: ReplaceOptions) -> Result<BTreeMap<Address, CellInput>> {
        ensure!(sheet < self.workbook.sheets.len(), "worksheet does not exist");
        ensure!(!needle.is_empty() && needle.len() <= 4096 && replacement.len() <= 32_767, "invalid replacement text");
        let pattern = regex::RegexBuilder::new(&regex::escape(needle)).case_insensitive(!options.case_sensitive).build()?;
        let addresses = self.originals.keys().chain(self.edits.keys()).copied().filter(|address| address.sheet == sheet).collect::<std::collections::BTreeSet<_>>();
        let mut edits = BTreeMap::new();
        let mut bytes = 0usize;
        for address in addresses {
            if mode == ReplaceMode::Formula && self.is_hidden_formula(address) { continue; }
            let current = self.input(address);
            let (input, is_date) = match (mode, &current) {
                (ReplaceMode::Text, Some(CellInput::Text(text))) => (text.clone(), false),
                (ReplaceMode::Formula, Some(CellInput::Formula(formula))) => (format!("={formula}"), false),
                (ReplaceMode::Values, Some(value @ CellInput::Number(number))) => match self.date_input_text_with_styles(address, value, styles) {
                    Some(date) => (date, true), None => (number.to_string(), false),
                },
                (ReplaceMode::Values, Some(CellInput::Boolean(value))) => ((if *value { "TRUE" } else { "FALSE" }).to_string(), false),
                _ => continue,
            };
            let matches = pattern.find_iter(&input).filter(|m| !options.whole_cell || (m.start() == 0 && m.end() == input.len())).collect::<Vec<_>>();
            if matches.is_empty() { continue; }
            let removed = matches.iter().map(|m| m.end() - m.start()).sum::<usize>();
            let result_len = input.len().checked_sub(removed).and_then(|n| n.checked_add(matches.len().checked_mul(replacement.len())?)).ok_or_else(|| anyhow::anyhow!("replacement size overflow"))?;
            bytes = bytes.checked_add(result_len).ok_or_else(|| anyhow::anyhow!("replacement size overflow"))?;
            ensure!(edits.len() < 100_000 && bytes <= 16 * 1024 * 1024, "replacement is too large");
            let mut output = String::with_capacity(result_len);
            let mut offset = 0;
            for found in matches { output.push_str(&input[offset..found.start()]); output.push_str(replacement); offset = found.end(); }
            output.push_str(&input[offset..]);
            let value = match mode {
                ReplaceMode::Text => CellInput::Text(output),
                ReplaceMode::Formula => CellInput::Formula(output.strip_prefix('=').ok_or_else(|| anyhow::anyhow!("replacement must retain the formula's leading equals sign"))?.to_string()),
                ReplaceMode::Values => {
                    let value = if is_date { self.parse_input_at_with_styles(address, &output, styles).map_err(|_| anyhow::anyhow!("replacement is not a valid date"))? } else { parse_input(&output) };
                    ensure!(matches!((&current, &value), (_, CellInput::Clear) | (Some(CellInput::Number(_)), CellInput::Number(_)) | (Some(CellInput::Boolean(_)), CellInput::Boolean(_))), "replacement is not a valid value of the same type");
                    value
                }
            };
            self.validate(address, &value)?;
            if self.input(address) != Some(value.clone()) { edits.insert(address, value); }
        }
        Ok(edits)
    }

    /// Preserve undo history across a sheet rename only if no stored formula
    /// could contain the old sheet name.
    pub fn history_mentions_sheet(&self, name: &str) -> bool {
        let needle = name.to_lowercase();
        self.undo.iter().chain(&self.redo).flatten().any(|change| match change {
            Change::Value { before, after, .. } => [before, after].iter().any(|value| matches!(value, CellInput::Formula(formula) if formula.replace("''", "'").to_lowercase().contains(&needle))),
            Change::Style { .. } => false,
        })
    }

    /// Rebase the overlay after a successful save without losing the user's
    /// undo/redo history. The new workbook must be the successfully saved data.
    pub fn mark_saved(&mut self, workbook: Workbook) {
        let fresh = Self::new(workbook);
        self.workbook = fresh.workbook;
        self.originals = fresh.originals;
        self.dates = fresh.dates;
        self.edits.clear();
        for (address, style) in std::mem::take(&mut self.style_edits) { self.style_originals.insert(address, style); }
    }

    /// Validate style history before the corresponding archive is published.
    pub fn validate_saved_styles(&self, changes: &BTreeMap<Address, (u32, u32)>) -> Result<()> {
        ensure!(!self.is_dirty(), "save pending cell edits before formatting");
        ensure!(changes.len() <= 100_000, "too many cells in one edit");
        for (&address, &(before, _)) in changes {
            ensure!(address.sheet < self.workbook.sheets.len() && address.row < 1_048_576 && address.column < 16_384, "cell is outside worksheet limits");
            ensure!(self.style_originals.get(&address).is_none_or(|value| *value == before), "cell style history is inconsistent");
        }
        Ok(())
    }

    /// Record a previously validated and successfully published formatting operation.
    pub fn record_saved_styles(&mut self, changes: &BTreeMap<Address, (u32, u32)>, workbook: Workbook) {
        let mut history = Vec::new();
        for (&address, &(before, after)) in changes {
            if before != after { history.push(Change::Style { address, before, after }); self.style_originals.insert(address, after); }
        }
        self.mark_saved(workbook);
        if !history.is_empty() { self.undo.push(history); self.redo.clear(); }
    }
    /// Return the pending or original editable value; `None` identifies a
    /// source type that cannot yet be safely edited (for example a date).
    pub fn input(&self, address: Address) -> Option<CellInput> {
        if address.sheet >= self.workbook.sheets.len() || address.row >= 1_048_576 || address.column >= 16_384 { return None; }
        self.edits.get(&address).cloned().or_else(|| self.original(address))
    }

    /// Editable text shown to the user. Protected hidden formula source must
    /// never enter a text field, clipboard, or search result.
    pub fn display_input(&self, address: Address) -> Option<CellInput> {
        if self.is_hidden_formula(address) { Some(CellInput::Clear) } else { self.input(address) }
    }

    /// Use an unambiguous calendar representation for existing date cells.
    /// Non-date and elapsed-time cells keep their ordinary editing notation.
    pub fn date_input_text(&self, address: Address, input: &CellInput) -> Option<String> {
        let is_1904 = *self.dates.get(&address)?;
        let CellInput::Number(value) = input else { return None };
        date_text(&calamine::ExcelDateTime::new(*value, calamine::ExcelDateTimeType::DateTime, is_1904))
    }

    pub fn date_input_text_with_styles(&self, address: Address, input: &CellInput, styles: &crate::spreadsheet_calc::PercentageStyles) -> Option<String> {
        let is_1904 = styles.date_epoch(&address).or_else(|| self.dates.get(&address).copied())?;
        let CellInput::Number(value) = input else { return None };
        date_text(&calamine::ExcelDateTime::new(*value, calamine::ExcelDateTimeType::DateTime, is_1904))
    }

    pub fn parse_input_at(&self, address: Address, text: &str, percentage: Option<bool>) -> Result<CellInput> {
        if let Some(&is_1904) = self.dates.get(&address) && looks_like_iso_date(text) {
            return Ok(CellInput::Number(parse_iso_date(text, is_1904)?));
        }
        parse_input_with_format(text, percentage)
    }

    pub fn parse_input_at_with_styles(&self, address: Address, text: &str, styles: &crate::spreadsheet_calc::PercentageStyles) -> Result<CellInput> {
        if let Some(is_1904) = styles.date_epoch(&address).or_else(|| self.dates.get(&address).copied()) && looks_like_iso_date(text) {
            return Ok(CellInput::Number(parse_iso_date(text, is_1904)?));
        }
        ensure!(!looks_like_iso_date(text) || styles.get(&address).is_some(), "cell format is unavailable; date-like input cannot be interpreted safely");
        parse_input_with_format(text, styles.get(&address))
    }

    pub fn set(&mut self, address: Address, value: CellInput) -> Result<()> {
        self.set_many([(address, value)])
    }

    /// Apply a batch atomically. A single undo restores every affected cell.
    pub fn set_many(&mut self, edits: impl IntoIterator<Item = (Address, CellInput)>) -> Result<()> {
        let mut unique = BTreeMap::new();
        for (address, value) in edits {
            ensure!(unique.len() < 100_000, "too many cells in one edit");
            self.validate(address, &value)?;
            ensure!(unique.insert(address, value).is_none(), "duplicate cell in edit");
        }
        let mut changes = Vec::new();
        for (address, value) in unique {
            let before = self.input(address).ok_or_else(|| anyhow::anyhow!("cell type cannot be edited safely"))?;
            if before != value { changes.push(Change::Value { address, before, after: value }); }
        }
        if changes.is_empty() { return Ok(()); }
        for change in &changes { self.apply_change(change, true); }
        self.undo.push(changes);
        self.redo.clear();
        Ok(())
    }

    fn validate(&self, address: Address, value: &CellInput) -> Result<()> {
        ensure!(address.sheet < self.workbook.sheets.len(), "worksheet does not exist");
        ensure!(address.row < 1_048_576 && address.column < 16_384, "cell is outside Excel's worksheet limits");
        ensure!(!self.is_hidden_formula(address), "hidden protected formula cannot be edited");
        match value {
            CellInput::Number(n) => ensure!(n.is_finite(), "cell number must be finite"),
            CellInput::Text(s) => ensure!(s.chars().count() <= 32_767, "cell text exceeds Excel's limit"),
            CellInput::Formula(s) => ensure!(!s.trim().is_empty() && s.chars().count() <= 8_192 && !s.starts_with('='), "invalid formula"),
            _ => {}
        }
        Ok(())
    }

    /// Plan an Excel-style arithmetic operation on a selection. Calendar
    /// dates, text, booleans and blank cells are left untouched. Formulas are
    /// adjusted only when explicitly requested.
    pub fn plan_arithmetic(&self, a: Address, b: Address, operation: Arithmetic, operand: f64, include_formulas: bool, styles: &crate::spreadsheet_calc::PercentageStyles) -> Result<BTreeMap<Address, CellInput>> {
        ensure!(a.sheet == b.sheet && a.sheet < self.workbook.sheets.len(), "invalid worksheet selection");
        ensure!(operand.is_finite(), "invalid arithmetic operand");
        ensure!(operation != Arithmetic::Divide || operand != 0.0, "cannot divide by zero");
        let (top, bottom) = (a.row.min(b.row), a.row.max(b.row));
        let (left, right) = (a.column.min(b.column), a.column.max(b.column));
        ensure!(bottom < 1_048_576 && right < 16_384, "selection outside worksheet limits");
        ensure!(u64::from(bottom - top + 1) * u64::from(right - left + 1) <= 100_000, "selection is too large");
        let mut result = BTreeMap::new();
        let mut bytes = 0usize;
        for row in top..=bottom {
            for column in left..=right {
                let address = Address { sheet: a.sheet, row, column };
                let Some(before) = self.input(address) else { anyhow::bail!("selection contains a cell that cannot be edited safely") };
                if styles.date_epoch(&address).is_some() || self.dates.contains_key(&address) { continue; }
                let after = match &before {
                    CellInput::Number(n) => {
                        ensure!(styles.get(&address).is_some(), "cell format is unavailable for arithmetic");
                        let value = match operation { Arithmetic::Add => n + operand, Arithmetic::Subtract => n - operand, Arithmetic::Multiply => n * operand, Arithmetic::Divide => n / operand };
                        ensure!(value.is_finite() && !(value == 0.0 && *n != 0.0 && operand != 0.0 && matches!(operation, Arithmetic::Multiply | Arithmetic::Divide)), "arithmetic result is outside the supported range");
                        CellInput::Number(value)
                    }
                    CellInput::Formula(formula) if include_formulas => {
                        ensure!(styles.get(&address).is_some(), "cell format is unavailable for arithmetic");
                        CellInput::Formula(format!("({formula}){}({operand})", operation.symbol()))
                    }
                    _ => continue,
                };
                self.validate(address, &after)?;
                if let CellInput::Formula(formula) = &after { bytes = bytes.checked_add(formula.len()).ok_or_else(|| anyhow::anyhow!("arithmetic size overflow"))?; }
                ensure!(bytes <= 16 * 1024 * 1024, "arithmetic content is too large");
                if before != after { result.insert(address, after); }
            }
        }
        Ok(result)
    }

    /// Paste tab-separated plain text, preserving empty cells and treating a
    /// final line ending as a terminator rather than an additional blank row.
    pub fn paste_tsv(&mut self, start: Address, text: &str) -> Result<()> {
        self.paste_tsv_using(start, text, None)
    }

    pub fn paste_tsv_with_formats(&mut self, start: Address, text: &str, percentage_styles: &crate::spreadsheet_calc::PercentageStyles) -> Result<()> {
        self.paste_tsv_using(start, text, Some(percentage_styles))
    }

    fn paste_tsv_using(&mut self, start: Address, text: &str, percentage_styles: Option<&crate::spreadsheet_calc::PercentageStyles>) -> Result<()> {
        ensure!(text.len() <= 16 * 1024 * 1024, "paste is too large");
        let mut edits = Vec::new();
        for (r, line) in parse_tsv(text)?.into_iter().enumerate() {
            for (c, input) in line.into_iter().enumerate() {
                ensure!(edits.len() < 100_000, "too many cells in one paste");
                let row = start.row.checked_add(u32::try_from(r)?).ok_or_else(|| anyhow::anyhow!("row overflow"))?;
                let column = start.column.checked_add(u32::try_from(c)?).ok_or_else(|| anyhow::anyhow!("column overflow"))?;
                let address = Address { sheet: start.sheet, row, column };
                let value = match percentage_styles { Some(styles) => self.parse_input_at_with_styles(address, &input, styles)?, None => parse_input(&input) };
                edits.push((address, value));
            }
        }
        self.set_many(edits)
    }

    /// Fill a rectangular selection as one undoable transaction. Formula
    /// translation is not implied; callers choose the exact cell input.
    pub fn fill_range(&mut self, a: Address, b: Address, value: CellInput) -> Result<()> {
        ensure!(a.sheet == b.sheet && a.sheet < self.workbook.sheets.len(), "invalid worksheet selection");
        let (top, bottom) = (a.row.min(b.row), a.row.max(b.row));
        let (left, right) = (a.column.min(b.column), a.column.max(b.column));
        ensure!(bottom < 1_048_576 && right < 16_384, "selection outside worksheet limits");
        let count = u64::from(bottom - top + 1) * u64::from(right - left + 1);
        ensure!(count <= 100_000, "selection is too large");
        let bytes = match &value { CellInput::Text(s) | CellInput::Formula(s) => s.len() as u64, _ => 0 };
        ensure!(bytes.saturating_mul(count) <= 16 * 1024 * 1024, "fill content is too large");
        self.set_many((top..=bottom).flat_map(|row| (left..=right).map(move |column| Address { sheet: a.sheet, row, column }))
            .map(|address| (address, value.clone())))
    }

    /// Translate relative references from the anchor into each destination.
    /// Refuse formulas the parser cannot reproduce exactly at their source.
    pub fn fill_formula(&mut self, a: Address, b: Address) -> Result<()> {
        use ironcalc::base::expressions::{parser::{new_parser_english, stringify::to_excel_string}, types::CellReferenceRC};
        ensure!(a.sheet == b.sheet && a.sheet < self.workbook.sheets.len(), "invalid worksheet selection");
        ensure!(!self.is_hidden_formula(a), "hidden protected formula cannot be copied");
        let (top, bottom) = (a.row.min(b.row), a.row.max(b.row));
        let (left, right) = (a.column.min(b.column), a.column.max(b.column));
        ensure!(bottom < 1_048_576 && right < 16_384, "selection outside worksheet limits");
        ensure!(u64::from(bottom - top + 1) * u64::from(right - left + 1) <= 100_000, "selection is too large");
        let Some(CellInput::Formula(formula)) = self.input(a) else { anyhow::bail!("anchor is not a formula") };
        let names = self.workbook.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        let sheet = names[a.sheet].clone();
        let source = CellReferenceRC { sheet: sheet.clone(), row: a.row as i32 + 1, column: a.column as i32 + 1 };
        let mut parser = new_parser_english(names, Vec::new(), std::collections::HashMap::new());
        let ast = parse_copy_formula(&mut parser, &formula, &source)?;
        let mut changes = Vec::new();
        let mut bytes = 0usize;
        for row in top..=bottom {
            for column in left..=right {
                let target = CellReferenceRC { sheet: sheet.clone(), row: row as i32 + 1, column: column as i32 + 1 };
                let translated = to_excel_string(&ast, &target);
                bytes = bytes.checked_add(translated.len()).ok_or_else(|| anyhow::anyhow!("fill size overflow"))?;
                ensure!(bytes <= 16 * 1024 * 1024, "fill content is too large");
                changes.push((Address { sheet: a.sheet, row, column }, CellInput::Formula(translated)));
            }
        }
        self.set_many(changes)
    }

    pub fn copy_cells(&self, a: Address, b: Address) -> Result<CellClipboard> {
        ensure!(a.sheet == b.sheet && a.sheet < self.workbook.sheets.len(), "invalid worksheet selection");
        let (top, bottom) = (a.row.min(b.row), a.row.max(b.row));
        let (left, right) = (a.column.min(b.column), a.column.max(b.column));
        ensure!(bottom < 1_048_576 && right < 16_384, "selection outside worksheet limits");
        let width = right - left + 1;
        let height = bottom - top + 1;
        ensure!(u64::from(width) * u64::from(height) <= 100_000, "selection is too large");
        let mut cells = Vec::new();
        let mut bytes = 0usize;
        for row in top..=bottom {
            for column in left..=right {
                let address = Address { sheet: a.sheet, row, column };
                ensure!(!self.is_hidden_formula(address), "hidden protected formula cannot be copied");
                let value = self.input(address).ok_or_else(|| anyhow::anyhow!("cell type cannot be copied safely"))?;
                if let CellInput::Text(s) | CellInput::Formula(s) = &value { bytes = bytes.saturating_add(s.len()); }
                ensure!(bytes <= 16 * 1024 * 1024, "copied selection is too large");
                cells.push(value);
            }
        }
        Ok(CellClipboard { sheet: a.sheet, row: top, column: left, width, height, cells })
    }

    /// Apply a typed clipboard atomically. Cross-workbook formula identity is
    /// checked by the caller before invoking this method.
    pub fn paste_cells(&mut self, destination: Address, clip: &CellClipboard) -> Result<()> {
        use ironcalc::base::expressions::{parser::{new_parser_english, stringify::to_excel_string}, types::CellReferenceRC};
        let count = u64::from(clip.width) * u64::from(clip.height);
        ensure!(clip.width > 0 && clip.height > 0 && count <= 100_000 && count == clip.cells.len() as u64, "invalid clipboard dimensions");
        ensure!(destination.sheet < self.workbook.sheets.len(), "invalid worksheet selection");
        ensure!(clip.row.checked_add(clip.height).is_some_and(|v| v <= 1_048_576) && clip.column.checked_add(clip.width).is_some_and(|v| v <= 16_384), "clipboard source outside worksheet limits");
        ensure!(destination.row.checked_add(clip.height).is_some_and(|v| v <= 1_048_576) && destination.column.checked_add(clip.width).is_some_and(|v| v <= 16_384), "clipboard destination outside worksheet limits");
        let has_formula = clip.cells.iter().any(|cell| matches!(cell, CellInput::Formula(_)));
        ensure!(!has_formula || clip.sheet < self.workbook.sheets.len(), "formula source worksheet is unavailable");
        let names = self.workbook.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        let source_sheet = names.get(clip.sheet).cloned().unwrap_or_default();
        let target_sheet = names[destination.sheet].clone();
        let mut parser = new_parser_english(names, Vec::new(), std::collections::HashMap::new());
        let mut changes = Vec::with_capacity(clip.cells.len());
        let mut bytes = 0usize;
        for (index, cell) in clip.cells.iter().enumerate() {
            let row = index as u32 / clip.width;
            let column = index as u32 % clip.width;
            let address = Address { sheet: destination.sheet, row: destination.row + row, column: destination.column + column };
            let value = if let CellInput::Formula(formula) = cell {
                let source = CellReferenceRC { sheet: source_sheet.clone(), row: (clip.row + row) as i32 + 1, column: (clip.column + column) as i32 + 1 };
                let target = CellReferenceRC { sheet: target_sheet.clone(), row: address.row as i32 + 1, column: address.column as i32 + 1 };
                let ast = parse_copy_formula(&mut parser, formula, &source)?;
                CellInput::Formula(to_excel_string(&ast, &target))
            } else { cell.clone() };
            if let CellInput::Text(s) | CellInput::Formula(s) = &value { bytes = bytes.saturating_add(s.len()); }
            ensure!(bytes <= 16 * 1024 * 1024, "paste is too large");
            changes.push((address, value));
        }
        self.set_many(changes)
    }

    /// Copy displayed values as TSV. Formula cells require a current local
    /// result; a stale or unavailable result is never silently copied.
    pub fn copy_values_tsv(&self, a: Address, b: Address, results: &BTreeMap<Address, crate::spreadsheet_calc::FormulaResult>) -> Result<String> {
        use crate::spreadsheet_calc::FormulaResult;
        ensure!(a.sheet == b.sheet && a.sheet < self.workbook.sheets.len(), "invalid worksheet selection");
        let (top, bottom) = (a.row.min(b.row), a.row.max(b.row));
        let (left, right) = (a.column.min(b.column), a.column.max(b.column));
        ensure!(bottom < 1_048_576 && right < 16_384, "selection outside worksheet limits");
        ensure!(u64::from(bottom - top + 1) * u64::from(right - left + 1) <= 100_000, "selection is too large");
        let source = self.workbook.sheets[a.sheet].cells.iter().filter(|cell| (top..=bottom).contains(&cell.row) && (left..=right).contains(&cell.column))
            .map(|cell| ((cell.row, cell.column), &cell.value)).collect::<BTreeMap<_, _>>();
        let mut text = String::new();
        for row in top..=bottom {
            if row != top { text.push('\n'); }
            for column in left..=right {
                if column != left { text.push('\t'); }
                let address = Address { sheet: a.sheet, row, column };
                let value = match self.input(address) {
                    Some(CellInput::Clear) => String::new(),
                    Some(CellInput::Text(s)) => s,
                    Some(CellInput::Number(n)) => n.to_string(),
                    Some(CellInput::Boolean(b)) => if b { "TRUE" } else { "FALSE" }.to_string(),
                    Some(CellInput::Formula(_)) => match results.get(&address).ok_or_else(|| anyhow::anyhow!("formula value is not available"))? {
                        FormulaResult::Empty => String::new(), FormulaResult::Number(n) => n.to_string(),
                        FormulaResult::Text(s) | FormulaResult::Error(s) => s.clone(),
                        FormulaResult::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                    },
                    None => source.get(&(row, column)).map_or_else(String::new, |value| value.to_string()),
                };
                ensure!(text.len().saturating_add(value.len().saturating_mul(2)).saturating_add(2) <= 16 * 1024 * 1024, "copied selection is too large");
                if value.contains(['\t', '\n', '\r', '"']) {
                    text.push('"');
                    for c in value.chars() { if c == '"' { text.push('"'); } text.push(c); }
                    text.push('"');
                } else { text.push_str(&value); }
            }
        }
        Ok(text)
    }

    /// Find a matching editable cell in worksheet order, wrapping once.
    /// Formula text and current calculated results are both searchable.
    pub fn find_cell(&self, sheet: usize, query: &str, after: Option<(u32, u32)>, reverse: bool, results: &BTreeMap<Address, crate::spreadsheet_calc::FormulaResult>, formatted: &BTreeMap<Address, String>) -> Option<Address> {
        use crate::spreadsheet_calc::FormulaResult;
        if sheet >= self.workbook.sheets.len() || query.is_empty() || query.len() > 4096 { return None; }
        let query = query.to_lowercase();
        let mut candidate: Option<Address> = None;
        let mut wrap: Option<Address> = None;
        for &address in self.originals.keys().chain(self.edits.keys()).filter(|a| a.sheet == sheet) {
            let value = self.input(address);
            let matches_display = value.as_ref().is_some_and(|value| !matches!(value, CellInput::Clear)) && formatted.get(&address).is_some_and(|text| text.to_lowercase().contains(&query));
            let matches = matches_display || match value {
                Some(CellInput::Text(text)) => text.to_lowercase().contains(&query),
                Some(CellInput::Number(number)) => number.to_string().contains(&query),
                Some(CellInput::Boolean(value)) => (if value { "true" } else { "false" }).contains(&query),
                Some(CellInput::Formula(formula)) => (!self.is_hidden_formula(address) && format!("={formula}").to_lowercase().contains(&query)) || results.get(&address).is_some_and(|value| {
                    let text = match value {
                        FormulaResult::Empty => String::new(), FormulaResult::Number(n) => n.to_string(),
                        FormulaResult::Text(s) | FormulaResult::Error(s) => s.clone(),
                        FormulaResult::Boolean(b) => b.to_string(),
                    };
                    text.to_lowercase().contains(&query)
                }),
                _ => false,
            };
            if !matches { continue; }
            let better = |current: Option<Address>| current.is_none_or(|old| if reverse { address > old } else { address < old });
            if better(wrap) { wrap = Some(address); }
            let beyond = after.is_none_or(|pos| if reverse { (address.row, address.column) < pos } else { (address.row, address.column) > pos });
            if beyond && better(candidate) { candidate = Some(address); }
        }
        candidate.or(wrap)
    }

    pub fn undo(&mut self) -> bool {
        let Some(changes) = self.undo.pop() else { return false };
        for change in changes.iter().rev() { self.apply_change(change, false); }
        self.redo.push(changes);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(changes) = self.redo.pop() else { return false };
        for change in &changes { self.apply_change(change, true); }
        self.undo.push(changes);
        true
    }

    fn apply(&mut self, address: Address, value: CellInput) {
        if self.original(address).as_ref() == Some(&value) { self.edits.remove(&address); }
        else { self.edits.insert(address, value); }
    }

    fn apply_change(&mut self, change: &Change, forward: bool) {
        match change {
            Change::Value { address, before, after } => self.apply(*address, if forward { after.clone() } else { before.clone() }),
            Change::Style { address, before, after } => {
                let value = if forward { *after } else { *before };
                if self.style_originals.get(address) == Some(&value) { self.style_edits.remove(address); }
                else { self.style_edits.insert(*address, value); }
            }
        }
    }

    fn original(&self, address: Address) -> Option<CellInput> {
        self.originals.get(&address).cloned().unwrap_or(Some(CellInput::Clear))
    }

}

/// Safe fallback when a date is available but the formula formatter is not.
pub fn date_text(date: &calamine::ExcelDateTime) -> Option<String> {
    if !date.is_datetime() || !(0.0..2_958_466.0).contains(&date.as_f64()) { return None; }
    let (year, month, day, hour, minute, second, milli) = date.to_ymd_hms_milli();
    if year > 9999 { return None; }
    let mut text = format!("{year:04}-{month:02}-{day:02}");
    if hour != 0 || minute != 0 || second != 0 || milli != 0 {
        text.push_str(&format!(" {hour:02}:{minute:02}:{second:02}"));
        if milli != 0 { text.push_str(&format!(".{milli:03}")); }
    }
    Some(text)
}

pub fn fallback_cell_text(cell: &crate::spreadsheet::Cell) -> String {
    match &cell.value {
        calamine::Data::DateTime(date) => date_text(date).unwrap_or_else(|| cell.value.to_string()),
        _ => cell.value.to_string(),
    }
}

/// Parse Excel-compatible tabular plain text, including quoted tabs, quoted
/// newlines and doubled quotes. Reject malformed quoting without applying any
/// part of the paste.
fn parse_tsv(text: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut closed = false;
    let mut at_start = true;
    let mut ended_row = false;
    let mut count = 0usize;
    while let Some(ch) = chars.next() {
        ended_row = false;
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') { chars.next(); field.push('"'); } else { quoted = false; closed = true; }
            } else { field.push(ch); }
            continue;
        }
        if at_start && ch == '"' { quoted = true; at_start = false; continue; }
        if ch == '\t' || ch == '\n' || ch == '\r' {
            count += 1;
            ensure!(count <= 100_000, "too many cells in one paste");
            row.push(std::mem::take(&mut field));
            at_start = true;
            closed = false;
            if ch != '\t' {
                if ch == '\r' && chars.peek() == Some(&'\n') { chars.next(); }
                rows.push(std::mem::take(&mut row));
                ended_row = true;
            }
        } else {
            ensure!(!closed, "unexpected character after quoted cell");
            field.push(ch);
            at_start = false;
        }
    }
    ensure!(!quoted, "unterminated quoted cell");
    if !ended_row {
        ensure!(count < 100_000, "too many cells in one paste");
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

fn original_cell(cell: &crate::spreadsheet::Cell) -> Option<CellInput> {
    use calamine::Data;
    if let Some(formula) = &cell.formula { return Some(CellInput::Formula(formula.clone())); }
    match &cell.value {
        Data::Empty => Some(CellInput::Clear),
        Data::String(s) => Some(CellInput::Text(s.clone())),
        Data::Bool(b) => Some(CellInput::Boolean(*b)),
        Data::Float(n) if n.is_finite() => Some(CellInput::Number(*n)),
        Data::DateTime(n) if n.as_f64().is_finite() => Some(CellInput::Number(n.as_f64())),
        Data::Int(n) if n.unsigned_abs() <= (1u64 << 53) => Some(CellInput::Number(*n as f64)),
        _ => None,
    }
}

fn looks_like_iso_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 10 && bytes[0..4].iter().all(u8::is_ascii_digit) && bytes[4] == b'-' && bytes[5..7].iter().all(u8::is_ascii_digit) && bytes[7] == b'-' && bytes[8..10].iter().all(u8::is_ascii_digit)
}

fn parse_iso_date(text: &str, is_1904: bool) -> Result<f64> {
    use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
    let (date, time) = text.split_once([' ', 'T']).map_or((text, None), |(date, time)| (date, Some(time)));
    // Excel's 1900 system deliberately contains the fictitious 1900-02-29.
    let fictitious = !is_1904 && date == "1900-02-29";
    let parsed_date = NaiveDate::parse_from_str(if fictitious { "1900-02-28" } else { date }, "%Y-%m-%d")?;
    let datetime = match time {
        None => parsed_date.and_hms_opt(0, 0, 0).unwrap(),
        Some(time) => NaiveDateTime::parse_from_str(&format!("{} {time}", parsed_date), "%Y-%m-%d %H:%M:%S%.f")
            .or_else(|_| NaiveDateTime::parse_from_str(&format!("{} {time}", parsed_date), "%Y-%m-%d %H:%M"))?,
    };
    let epoch = NaiveDate::from_ymd_opt(if is_1904 { 1904 } else { 1899 }, if is_1904 { 1 } else { 12 }, if is_1904 { 1 } else { 31 }).unwrap();
    let mut days = (parsed_date - epoch).num_days();
    if !is_1904 && (fictitious || parsed_date >= NaiveDate::from_ymd_opt(1900, 3, 1).unwrap()) { days += 1; }
    ensure!(days >= 0 && parsed_date.year() <= 9999, "date outside Excel's range");
    let nanos = u64::from(datetime.time().num_seconds_from_midnight()) * 1_000_000_000 + u64::from(datetime.time().nanosecond());
    Ok(days as f64 + nanos as f64 / 86_400_000_000_000.0)
}

/// Excel-style plain input; a leading apostrophe explicitly forces text.
pub fn parse_input(text: &str) -> CellInput {
    if text.is_empty() { return CellInput::Clear; }
    if let Some(value) = text.strip_prefix('\'') { return CellInput::Text(value.to_string()); }
    if let Some(value) = text.strip_prefix('=') { return CellInput::Formula(value.to_string()); }
    if text.eq_ignore_ascii_case("true") { return CellInput::Boolean(true); }
    if text.eq_ignore_ascii_case("false") { return CellInput::Boolean(false); }
    if text.trim() == text && let Ok(value) = text.parse::<f64>() && value.is_finite() {
        // Excel stores only 15 significant decimal digits. Preserve longer
        // identifiers as text instead of silently rounding game-data IDs.
        let mantissa = text.split(['e', 'E']).next().unwrap_or(text);
        let digits = mantissa.chars().filter(char::is_ascii_digit).skip_while(|c| *c == '0').count();
        if digits <= 15 { return CellInput::Number(value); }
    }
    CellInput::Text(text.to_string())
}

/// Percentage notation is converted only when the destination's existing
/// number format is known to be a percentage; otherwise no text is written.
pub fn parse_input_with_format(text: &str, percentage_style: Option<bool>) -> Result<CellInput> {
    if text.trim() == text && let Some(base) = text.strip_suffix('%') {
        let base = base.trim_end();
        if let Ok(number) = base.parse::<f64>() {
            ensure!(number.is_finite(), "invalid percentage number");
            let mantissa = base.split(['e', 'E']).next().unwrap_or(base);
            ensure!(number != 0.0 || !mantissa.chars().any(|c| ('1'..='9').contains(&c)), "percentage number is out of range");
            let CellInput::Number(number) = parse_input(base) else { anyhow::bail!("percentage number exceeds precision limit") };
            ensure!(percentage_style == Some(true), "percentage format is unavailable for this cell");
            let scaled = number / 100.0;
            ensure!(scaled.is_finite() && (number == 0.0 || scaled != 0.0), "percentage number is out of range");
            return Ok(CellInput::Number(scaled));
        }
    }
    Ok(parse_input(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spreadsheet::Sheet;

    #[test]
    fn arithmetic_selection_is_atomic_undoable_and_skips_dates() {
        let source = include_bytes!("../tests/fixtures/number_formats.xlsx");
        let styles = crate::spreadsheet_calc::evaluate(source).unwrap().percentage_styles;
        let mut session = Session::new(crate::spreadsheet::read_xlsx_snapshot(source).unwrap());
        let at = |row, column| Address { sheet: 0, row, column };
        let edits = session.plan_arithmetic(at(0, 0), at(3, 1), Arithmetic::Multiply, 2.0, false, &styles).unwrap();
        assert_eq!(edits.len(), 3);
        assert_eq!(edits[&at(0, 0)], CellInput::Number(0.25));
        assert_eq!(edits[&at(1, 0)], CellInput::Number(2469.0));
        assert!(!edits.contains_key(&at(3, 0)) && !edits.contains_key(&at(0, 1)));
        assert!(!session.is_dirty());
        session.set_many(edits).unwrap();
        assert!(session.undo());
        assert!(!session.is_dirty());
        assert!(session.redo());
        let updated = crate::spreadsheet_xlsx::patch(source, session.edits()).unwrap();
        let values = crate::spreadsheet_calc::evaluate(&updated).unwrap();
        assert_eq!(values.formatted[&at(0, 0)], "25.00%");
        assert_eq!(values.formatted[&at(3, 0)], "2025-01-02");
        assert_eq!(values.formatted[&at(0, 1)], "50.0%");

        assert_eq!(session.plan_arithmetic(at(0, 0), at(0, 0), Arithmetic::Add, 1.0, false, &styles).unwrap()[&at(0, 0)], CellInput::Number(1.25));
        assert_eq!(session.plan_arithmetic(at(0, 0), at(0, 0), Arithmetic::Subtract, 1.0, false, &styles).unwrap()[&at(0, 0)], CellInput::Number(-0.75));
        assert_eq!(session.plan_arithmetic(at(0, 0), at(0, 0), Arithmetic::Divide, 2.0, false, &styles).unwrap()[&at(0, 0)], CellInput::Number(0.125));
        let with_formula = session.plan_arithmetic(at(0, 1), at(0, 1), Arithmetic::Multiply, 2.0, true, &styles).unwrap();
        assert_eq!(with_formula[&at(0, 1)], CellInput::Formula("(A1*2)*(2)".into()));
        assert!(session.plan_arithmetic(at(0, 0), at(3, 1), Arithmetic::Divide, 0.0, true, &styles).is_err());
        assert!(session.plan_arithmetic(at(0, 0), at(3, 1), Arithmetic::Multiply, f64::MAX, true, &styles).is_err());
        assert!(session.plan_arithmetic(at(0, 0), at(0, 0), Arithmetic::Multiply, 2.0, false, &Default::default()).is_err());
        assert!(session.plan_arithmetic(at(0, 0), at(1_048_575, 1), Arithmetic::Multiply, 2.0, false, &styles).is_err());
    }

    #[test]
    fn arithmetic_keeps_1904_dates_and_formulas_untouched_by_default() {
        let source = include_bytes!("../tests/fixtures/dates_1904.xlsx");
        let styles = crate::spreadsheet_calc::evaluate(source).unwrap().percentage_styles;
        let mut session = Session::new(crate::spreadsheet::read_xlsx_snapshot(source).unwrap());
        let at = |column| Address { sheet: 0, row: 0, column };
        let edits = session.plan_arithmetic(at(0), at(3), Arithmetic::Multiply, 2.0, false, &styles).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[&at(3)], CellInput::Number(0.5));
        session.set_many(edits).unwrap();
        let updated = crate::spreadsheet_xlsx::patch(source, session.edits()).unwrap();
        let (book, is_1904) = crate::spreadsheet::read_xlsx_snapshot_with_epoch(&updated).unwrap();
        assert!(is_1904);
        assert_eq!(fallback_cell_text(&book.sheets[0].cells[0]), "2025-01-02");
        let evaluation = crate::spreadsheet_calc::evaluate(&updated).unwrap();
        assert!(evaluation.calculation_unavailable);
        assert_eq!(evaluation.formatted[&at(3)], "50.0%");
        assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 1 && cell.formula.as_deref() == Some("A1+1")));
    }

    #[test]
    fn replacement_plans_are_literal_scoped_atomic_and_undoable() {
        use calamine::Data;
        let cell = |row, column, value, formula| crate::spreadsheet::Cell { row, column, value, formula };
        let mut session = Session::new(Workbook { sheets: vec![
            Sheet { name: "balance".into(), cells: vec![
                cell(0, 0, Data::String("生命 HP HP".into()), None),
                cell(1, 0, Data::String("hp".into()), None),
                cell(2, 0, Data::Int(100), None),
                cell(3, 0, Data::Int(0), Some("A1+HP".into())),
            ] },
            Sheet { name: "other".into(), cells: vec![cell(0, 0, Data::String("HP".into()), None)] },
        ] });
        let address = |row| Address { sheet: 0, row, column: 0 };
        let plan = session.plan_replace_all(0, "HP", "体力", ReplaceMode::Text).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[&address(0)], CellInput::Text("生命 体力 体力".into()));
        assert!(!session.is_dirty());
        session.set_many(plan).unwrap();
        assert_eq!(session.input(address(0)), Some(CellInput::Text("生命 体力 体力".into())));
        assert_eq!(session.input(address(1)), Some(CellInput::Text("hp".into())));
        assert_eq!(session.input(address(2)), Some(CellInput::Number(100.0)));
        assert_eq!(session.input(address(3)), Some(CellInput::Formula("A1+HP".into())));
        assert!(session.undo());
        assert_eq!(session.input(address(0)), Some(CellInput::Text("生命 HP HP".into())));
        assert!(session.redo());
        let formula = session.plan_replace_all(0, "HP", "MP", ReplaceMode::Formula).unwrap();
        assert_eq!(formula[&address(3)], CellInput::Formula("A1+MP".into()));
        assert!(session.plan_replace_all(0, "=", "", ReplaceMode::Formula).is_err());
        assert!(session.plan_replace_all(0, "", "x", ReplaceMode::Text).is_err());
        assert!(session.plan_replace_all(0, "HP", &"x".repeat(32_768), ReplaceMode::Text).is_err());
        assert_eq!(session.input(Address { sheet: 1, row: 0, column: 0 }), Some(CellInput::Text("HP".into())));
    }

    #[test]
    fn replacement_options_handle_unicode_whole_cells_and_literal_dollars() {
        use calamine::Data;
        let cell = |row, text: &str| crate::spreadsheet::Cell { row, column: 0, value: Data::String(text.into()), formula: None };
        let session = Session::new(Workbook { sheets: vec![Sheet { name: "balance".into(), cells: vec![cell(0, "hp"), cell(1, "HP HP"), cell(2, "Hp"), cell(3, "Σ"), cell(4, "σ")] }] });
        let styles = Default::default();
        let options = ReplaceOptions { case_sensitive: false, whole_cell: false };
        let plan = session.plan_replace_all_with_options(0, "hp", "$1", ReplaceMode::Text, &styles, options).unwrap();
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[&Address { sheet: 0, row: 1, column: 0 }], CellInput::Text("$1 $1".into()));
        let plan = session.plan_replace_all_with_options(0, "hp", "x", ReplaceMode::Text, &styles, ReplaceOptions { whole_cell: true, ..options }).unwrap();
        assert_eq!(plan.len(), 2);
        let plan = session.plan_replace_all_with_options(0, "σ", "x", ReplaceMode::Text, &styles, options).unwrap();
        assert_eq!(plan.len(), 2);
        assert!(!session.is_dirty());
    }

    #[test]
    fn replace_raw_values_and_dates_retains_types_formats_and_epoch() {
        let source = include_bytes!("../tests/fixtures/number_formats.xlsx");
        let styles = crate::spreadsheet_calc::evaluate(source).unwrap().percentage_styles;
        let mut session = Session::new(crate::spreadsheet::read_xlsx_snapshot(source).unwrap());
        let a = Address { sheet: 0, row: 0, column: 0 };
        let date = Address { sheet: 0, row: 3, column: 0 };
        let options = ReplaceOptions { case_sensitive: false, whole_cell: true };
        let edits = session.plan_replace_all_with_options(0, "0.125", "0.5", ReplaceMode::Values, &styles, options).unwrap();
        assert_eq!(edits[&a], CellInput::Number(0.5));
        session.set_many(edits).unwrap();
        let edits = session.plan_replace_all_with_options(0, "2025-01-02", "2026-02-03", ReplaceMode::Values, &styles, options).unwrap();
        assert_eq!(edits[&date], CellInput::Number(46056.0));
        session.set_many(edits).unwrap();
        let updated = crate::spreadsheet_xlsx::patch(source, session.edits()).unwrap();
        let evaluated = crate::spreadsheet_calc::evaluate(&updated).unwrap();
        assert_eq!(evaluated.formatted[&a], "50.00%");
        assert_eq!(evaluated.formatted[&date], "2026-02-03");
        assert!(session.undo());
        assert!(session.undo());
        assert!(!session.is_dirty());
        let bad = session.plan_replace_all_with_options(0, "0.125", "not a number", ReplaceMode::Values, &styles, options);
        assert!(bad.is_err() && !session.is_dirty());
        let mixed = session.plan_replace_all_with_options(0, "2", "", ReplaceMode::Values, &styles, ReplaceOptions { whole_cell: false, ..options });
        assert!(mixed.is_err() && !session.is_dirty());
        let cleared = session.plan_replace_all_with_options(0, "0.125", "", ReplaceMode::Values, &styles, options).unwrap();
        assert_eq!(cleared[&a], CellInput::Clear);

        let source = include_bytes!("../tests/fixtures/dates_1904.xlsx");
        let styles = crate::spreadsheet_calc::evaluate(source).unwrap().percentage_styles;
        let mut session = Session::new(crate::spreadsheet::read_xlsx_snapshot(source).unwrap());
        let edits = session.plan_replace_all_with_options(0, "2025-01-02", "2025-01-03", ReplaceMode::Values, &styles, options).unwrap();
        assert_eq!(edits[&a], CellInput::Number(44198.0));
        session.set_many(edits).unwrap();
        let updated = crate::spreadsheet_xlsx::patch(source, session.edits()).unwrap();
        let (book, is_1904) = crate::spreadsheet::read_xlsx_snapshot_with_epoch(&updated).unwrap();
        assert!(is_1904);
        assert_eq!(fallback_cell_text(&book.sheets[0].cells[0]), "2025-01-03");

        let boolean = crate::spreadsheet::Cell { row: 0, column: 0, value: calamine::Data::Bool(true), formula: None };
        let session = Session::new(Workbook { sheets: vec![Sheet { name: "values".into(), cells: vec![boolean] }] });
        let edits = session.plan_replace_all_with_options(0, "true", "FALSE", ReplaceMode::Values, &Default::default(), options).unwrap();
        assert_eq!(edits[&a], CellInput::Boolean(false));
        assert!(session.plan_replace_all_with_options(0, "TRUE", "1", ReplaceMode::Values, &Default::default(), options).is_err());
    }

    #[test]
    fn existing_dates_edit_as_calendar_values_and_keep_epoch_and_undo() {
        use calamine::{Data, ExcelDateTime, ExcelDateTimeType};
        let cell = |column, value, epoch| crate::spreadsheet::Cell {
            row: 0, column, value: Data::DateTime(ExcelDateTime::new(value, ExcelDateTimeType::DateTime, epoch)), formula: None,
        };
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "Dates".into(), cells: vec![
            cell(0, 45659.0, false), cell(1, 44197.5, true), cell(2, 60.0, false),
        ] }] });
        let address = |column| Address { sheet: 0, row: 0, column };
        assert_eq!(session.date_input_text(address(0), &session.input(address(0)).unwrap()).as_deref(), Some("2025-01-02"));
        assert_eq!(session.date_input_text(address(1), &session.input(address(1)).unwrap()).as_deref(), Some("2025-01-02 12:00:00"));
        assert_eq!(session.date_input_text(address(2), &session.input(address(2)).unwrap()).as_deref(), Some("1900-02-29"));
        assert_eq!(session.parse_input_at(address(2), "1900-02-29", None).unwrap(), CellInput::Number(60.0));
        assert_eq!(session.parse_input_at(address(1), "2025-01-03 06:30", None).unwrap(), CellInput::Number(44198.270833333336));
        assert!(session.parse_input_at(address(0), "2025-02-29", None).is_err());
        assert!(session.parse_input_at(address(1), "1900-01-01", None).is_err());
        assert_eq!(session.parse_input_at(address(0), "'2025-02-29", None).unwrap(), CellInput::Text("2025-02-29".into()));
        let input = session.parse_input_at(address(0), "2025-01-03", None).unwrap();
        session.set(address(0), input).unwrap();
        assert_eq!(session.input(address(0)), Some(CellInput::Number(45660.0)));
        assert!(session.undo());
        assert_eq!(session.input(address(0)), Some(CellInput::Number(45659.0)));
        assert!(session.redo());
        let styles = crate::spreadsheet_calc::PercentageStyles::test_cells(&BTreeMap::from([(address(0), false)]));
        session.paste_tsv_with_formats(address(0), "2025-01-04\t2025-01-04 12:00", &styles).unwrap();
        assert_eq!(session.input(address(0)), Some(CellInput::Number(45661.0)));
        assert_eq!(session.input(address(1)), Some(CellInput::Number(44199.5)));
        assert!(session.undo());
        assert_eq!(session.input(address(0)), Some(CellInput::Number(45660.0)));
        assert_eq!(session.input(address(1)), Some(CellInput::Number(44197.5)));
    }

    #[test]
    fn real_date_cell_remains_a_date_after_edit_and_save() {
        let source = include_bytes!("../tests/fixtures/number_formats.xlsx");
        let workbook = crate::spreadsheet::read_xlsx_snapshot(source).unwrap();
        let mut session = Session::new(workbook);
        let address = Address { sheet: 0, row: 3, column: 0 };
        let input = session.parse_input_at(address, "2026-02-03", None).unwrap();
        session.set(address, input).unwrap();
        let updated = crate::spreadsheet_xlsx::patch(source, session.edits()).unwrap();
        let reloaded = crate::spreadsheet::read_xlsx_snapshot(&updated).unwrap();
        let cell = reloaded.sheets[0].cells.iter().find(|cell| cell.row == 3 && cell.column == 0).unwrap();
        let calamine::Data::DateTime(date) = cell.value else { panic!("date style was lost") };
        let (year, month, day, _, _, _, _) = date.to_ymd_hms_milli();
        assert_eq!((year, month, day), (2026, 2, 3));
    }

    #[test]
    fn paste_accepts_excel_quoted_cells_and_rejects_malformed_input() {
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "数值".into(), cells: vec![] }] });
        let start = Address { sheet: 0, row: 0, column: 0 };
        session.paste_tsv(start, "\"a\tb\"\t\"line1\nline2\"\r\n\"say \"\"hi\"\"\"\t尾\r\n").unwrap();
        assert_eq!(session.edits()[&start], CellInput::Text("a\tb".into()));
        assert_eq!(session.edits()[&Address { column: 1, ..start }], CellInput::Text("line1\nline2".into()));
        assert_eq!(session.edits()[&Address { row: 1, ..start }], CellInput::Text("say \"hi\"".into()));
        let before = session.edits().clone();
        assert!(session.paste_tsv(start, "\"unterminated").is_err());
        assert!(session.paste_tsv(start, "\"closed\"x").is_err());
        assert_eq!(session.edits(), &before);
    }

    #[test]
    fn sparse_edits_undo_redo_and_validation() {
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "Values".into(), cells: vec![] }] });
        let a = Address { sheet: 0, row: 1_048_575, column: 16_383 };
        let b = Address { sheet: 0, row: 0, column: 0 };
        session.set(a, CellInput::Text("生命值".into())).unwrap();
        session.set(b, CellInput::Formula("SUM(B1:B4)".into())).unwrap();
        assert!(session.is_dirty());
        assert_eq!(session.edits().len(), 2);
        assert!(session.undo());
        assert_eq!(session.edits().len(), 1);
        assert!(session.redo());
        assert_eq!(session.edits()[&b], CellInput::Formula("SUM(B1:B4)".into()));
        assert!(session.set(b, CellInput::Number(f64::NAN)).is_err());
        assert!(session.set(Address { row: 1_048_576, ..a }, CellInput::Clear).is_err());
        assert!(session.undo());
        session.set(b, CellInput::Number(42.0)).unwrap();
        assert!(!session.redo());
        assert!(session.undo());
        assert!(session.undo());
        assert!(!session.is_dirty());
    }

    #[test]
    fn restoring_the_original_value_clears_the_pending_change() {
        use calamine::Data;
        use crate::spreadsheet::Cell;
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "Values".into(), cells: vec![Cell { row: 0, column: 0, value: Data::Float(12.0), formula: None }] }] });
        let address = Address { sheet: 0, row: 0, column: 0 };
        session.set(address, CellInput::Number(99.0)).unwrap();
        session.set(address, CellInput::Number(12.0)).unwrap();
        assert!(!session.is_dirty());
        assert!(session.undo());
        assert_eq!(session.edits()[&address], CellInput::Number(99.0));
        assert!(session.redo());
        assert!(!session.is_dirty());
    }

    #[test]
    fn paste_is_atomic_and_undoes_as_one_action() {
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "数值".into(), cells: vec![] }] });
        let start = Address { sheet: 0, row: 3, column: 2 };
        session.paste_tsv(start, "'001\t42\t=SUM(A1:A2)\r\nTRUE\t\t技能\r\n").unwrap();
        assert_eq!(session.edits().len(), 5);
        assert_eq!(session.edits()[&start], CellInput::Text("001".into()));
        assert_eq!(session.edits()[&Address { column: 3, ..start }], CellInput::Number(42.0));
        assert_eq!(session.edits()[&Address { column: 4, ..start }], CellInput::Formula("SUM(A1:A2)".into()));
        assert!(session.undo());
        assert!(!session.is_dirty());
        assert!(session.redo());
        assert_eq!(session.edits().len(), 5);
        let before = session.edits().clone();
        assert!(session.paste_tsv(Address { row: 1_048_575, ..start }, "1\n2").is_err());
        assert_eq!(session.edits(), &before);
        assert_eq!(parse_input("123456789012345678"), CellInput::Text("123456789012345678".into()));
        assert_eq!(parse_input("1.25e3"), CellInput::Number(1250.0));
        assert!(session.paste_tsv(start, "=").is_err());
        assert_eq!(session.edits(), &before);
    }

    #[test]
    fn percentage_input_requires_known_percentage_format_and_paste_is_atomic() {
        assert_eq!(parse_input_with_format("50%", Some(true)).unwrap(), CellInput::Number(0.5));
        assert_eq!(parse_input_with_format("-12.5 %", Some(true)).unwrap(), CellInput::Number(-0.125));
        assert_eq!(parse_input_with_format("'50%", None).unwrap(), CellInput::Text("50%".into()));
        let text_clip = CellClipboard { sheet: 0, row: 0, column: 0, width: 1, height: 1, cells: vec![CellInput::Text("50%".into())] };
        assert_eq!(text_clip.plain_tsv().unwrap(), "'50%");
        let tiny_text = CellClipboard { cells: vec![CellInput::Text("1e-400%".into())], ..text_clip };
        assert_eq!(tiny_text.plain_tsv().unwrap(), "'1e-400%");
        assert_eq!(parse_input_with_format("HP +50%", None).unwrap(), CellInput::Text("HP +50%".into()));
        assert!(parse_input_with_format("50%", None).is_err());
        assert!(parse_input_with_format("50%", Some(false)).is_err());
        assert!(parse_input_with_format("1234567890123456%", Some(true)).is_err());
        assert!(parse_input_with_format("1e-323%", Some(true)).is_err());
        assert!(parse_input_with_format("1e-400%", Some(true)).is_err());
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "数值".into(), cells: vec![] }] });
        let a = Address { sheet: 0, row: 0, column: 0 };
        let b = Address { column: 1, ..a };
        let mut formats = BTreeMap::from([(a, true), (b, false)]);
        assert!(session.paste_tsv_with_formats(a, "50%\t25%", &crate::spreadsheet_calc::PercentageStyles::test_cells(&formats)).is_err());
        assert!(!session.is_dirty());
        formats.insert(b, true);
        session.paste_tsv_with_formats(a, "50%\t25%", &crate::spreadsheet_calc::PercentageStyles::test_cells(&formats)).unwrap();
        assert_eq!(session.input(a), Some(CellInput::Number(0.5)));
        assert_eq!(session.input(b), Some(CellInput::Number(0.25)));
        assert!(session.undo() && !session.is_dirty());
    }

    #[test]
    fn copies_rectangular_values_with_excel_tsv_quoting() {
        use crate::spreadsheet_calc::FormulaResult;
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "数值".into(), cells: vec![] }] });
        let start = Address { sheet: 0, row: 0, column: 0 };
        session.paste_tsv(start, "\"a\tb\"\t\"line1\nline2\"\n\"say \"\"hi\"\"\"\t=SUM(1,2)").unwrap();
        let end = Address { row: 1, column: 1, ..start };
        assert!(session.copy_values_tsv(start, end, &BTreeMap::new()).is_err());
        let results = BTreeMap::from([(end, FormulaResult::Number(3.0))]);
        let text = session.copy_values_tsv(end, start, &results).unwrap();
        assert_eq!(parse_tsv(&text).unwrap(), vec![vec!["a\tb", "line1\nline2"], vec!["say \"hi\"", "3"]]);
        assert!(session.copy_values_tsv(start, Address { row: 1000, column: 1000, ..start }, &results).is_err());
        assert!(session.copy_values_tsv(start, Address { sheet: 1, ..start }, &results).is_err());
    }

    #[test]
    fn fills_and_clears_ranges_atomically_with_undo() {
        use calamine::Data;
        use crate::spreadsheet::Cell;
        let start = Address { sheet: 0, row: 0, column: 0 };
        let end = Address { row: 1, column: 1, ..start };
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "数值".into(), cells: vec![] }] });
        session.fill_range(end, start, CellInput::Number(7.0)).unwrap();
        assert_eq!(session.edits().len(), 4);
        assert!(session.undo());
        assert!(!session.is_dirty());
        assert!(session.redo());
        session.fill_range(start, end, CellInput::Clear).unwrap();
        assert!(!session.is_dirty());
        assert!(session.undo());
        assert_eq!(session.edits().len(), 4);
        let before = session.edits().clone();
        assert!(session.fill_range(start, Address { row: 1_048_576, ..start }, CellInput::Clear).is_err());
        assert!(session.fill_range(start, Address { row: 1000, column: 1000, ..start }, CellInput::Clear).is_err());
        assert!(session.fill_range(start, Address { row: 199, column: 99, ..start }, CellInput::Text("x".repeat(1000))).is_err());
        assert_eq!(session.edits(), &before);
        let mut unsupported = Session::new(Workbook { sheets: vec![Sheet { name: "data".into(), cells: vec![Cell { row: 1, column: 1, value: Data::Error(calamine::CellErrorType::Div0), formula: None }] }] });
        assert!(unsupported.fill_range(start, end, CellInput::Clear).is_err());
        assert!(!unsupported.is_dirty() && !unsupported.can_undo());
    }

    #[test]
    fn fills_relative_and_absolute_formula_references() {
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "数值".into(), cells: vec![] }, Sheet { name: "Other Sheet".into(), cells: vec![] }] });
        let a = Address { sheet: 0, row: 1, column: 1 };
        let end = Address { row: 2, column: 2, ..a };
        session.set(a, CellInput::Formula("A1+$B2+C$1+$D$4".into())).unwrap();
        session.fill_formula(a, end).unwrap();
        assert_eq!(session.input(end), Some(CellInput::Formula("B2+$B3+D$1+$D$4".into())));
        assert!(session.undo());
        assert_eq!(session.input(end), Some(CellInput::Clear));
        assert_eq!(session.input(a), Some(CellInput::Formula("A1+$B2+C$1+$D$4".into())));
        assert!(session.redo());
        let before = session.edits().clone();
        assert!(session.fill_formula(a, Address { row: 1_048_576, ..a }).is_err());
        assert_eq!(session.edits(), &before);
        session.set(a, CellInput::Formula("SUM('Other Sheet'!A1:B2)+IF(A1=\"A1\",1,0)".into())).unwrap();
        session.fill_formula(a, end).unwrap();
        assert_eq!(session.input(end), Some(CellInput::Formula("SUM('Other Sheet'!B2:C3)+IF(B2=\"A1\",1,0)".into())));
        session.set(a, CellInput::Formula("A1".into())).unwrap();
        session.fill_formula(a, Address { row: 0, column: 0, ..a }).unwrap();
        assert_eq!(session.input(Address { row: 0, column: 0, ..a }), Some(CellInput::Formula("#REF!".into())));
        session.set(a, CellInput::Formula("A1+".into())).unwrap();
        let before = session.edits().clone();
        assert!(session.fill_formula(a, end).is_err());
        assert_eq!(session.edits(), &before);
        session.set(a, CellInput::Formula("SUM(Table1[Attack])".into())).unwrap();
        let before = session.edits().clone();
        assert!(session.fill_formula(a, end).is_err());
        assert_eq!(session.edits(), &before);
    }

    #[test]
    fn searches_pending_values_and_formula_results_in_both_directions() {
        use crate::spreadsheet_calc::FormulaResult;
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "data".into(), cells: vec![] }, Sheet { name: "other".into(), cells: vec![] }] });
        let a = Address { sheet: 0, row: 0, column: 0 };
        let b = Address { row: 2, column: 1, ..a };
        let c = Address { row: 4, column: 0, ..a };
        session.set(a, CellInput::Text("Attack 生命".into())).unwrap();
        session.set(b, CellInput::Formula("SUM(A1:A2)".into())).unwrap();
        session.set(c, CellInput::Text("ATTACK speed".into())).unwrap();
        let results = BTreeMap::from([(b, FormulaResult::Number(42.0))]);
        let formatted = BTreeMap::from([(b, "4,200%".into())]);
        assert_eq!(session.find_cell(0, "attack", None, false, &results, &formatted), Some(a));
        assert_eq!(session.find_cell(0, "attack", Some((0, 0)), false, &results, &formatted), Some(c));
        assert_eq!(session.find_cell(0, "attack", Some((4, 0)), false, &results, &formatted), Some(a));
        assert_eq!(session.find_cell(0, "attack", Some((0, 0)), true, &results, &formatted), Some(c));
        assert_eq!(session.find_cell(0, "sum(", None, false, &results, &formatted), Some(b));
        assert_eq!(session.find_cell(0, "=sum(", None, false, &results, &formatted), Some(b));
        assert_eq!(session.find_cell(0, "42", None, false, &results, &formatted), Some(b));
        assert_eq!(session.find_cell(0, "4,200%", None, false, &results, &formatted), Some(b));
        assert_eq!(session.find_cell(1, "attack", None, false, &results, &formatted), None);
        session.set(a, CellInput::Clear).unwrap();
        assert_eq!(session.find_cell(0, "生命", None, false, &results, &formatted), None);
    }

    #[test]
    fn typed_clipboard_pastes_formulas_types_and_blanks_atomically() {
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "数值".into(), cells: vec![] }, Sheet { name: "other".into(), cells: vec![] }] });
        let source = Address { sheet: 0, row: 1, column: 1 };
        let formula = Address { column: 2, ..source };
        session.set(source, CellInput::Text("001".into())).unwrap();
        session.set(formula, CellInput::Formula("A1+$A1+B$1+$C$1".into())).unwrap();
        session.set(Address { row: 2, ..source }, CellInput::Boolean(true)).unwrap();
        let clip = session.copy_cells(source, Address { row: 2, column: 2, ..source }).unwrap();
        assert!(clip.plain_tsv().unwrap().starts_with("'001\t=A1"));
        let clip: CellClipboard = serde_json::from_str(&serde_json::to_string(&clip).unwrap()).unwrap();
        let destination = Address { sheet: 0, row: 3, column: 3 };
        let cleared = Address { row: 4, column: 4, ..destination };
        session.set(cleared, CellInput::Text("old".into())).unwrap();
        session.paste_cells(destination, &clip).unwrap();
        assert_eq!(session.input(destination), Some(CellInput::Text("001".into())));
        assert_eq!(session.input(Address { column: 4, ..destination }), Some(CellInput::Formula("C3+$A3+D$1+$C$1".into())));
        assert_eq!(session.input(Address { row: 4, ..destination }), Some(CellInput::Boolean(true)));
        assert_eq!(session.input(cleared), Some(CellInput::Clear));
        assert!(session.undo());
        assert_eq!(session.input(cleared), Some(CellInput::Text("old".into())));
        assert_eq!(session.input(destination), Some(CellInput::Clear));
        assert!(session.redo());
        session.paste_cells(Address { sheet: 1, ..destination }, &clip).unwrap();
        assert_eq!(session.input(Address { sheet: 1, column: 4, ..destination }), Some(CellInput::Formula("C3+$A3+D$1+$C$1".into())));
        assert!(session.undo());
        let before = session.edits().clone();
        let missing_sheet = CellClipboard { sheet: 7, ..clip.clone() };
        assert!(session.paste_cells(destination, &missing_sheet).is_err());
        assert!(session.paste_cells(Address { row: 1_048_575, ..destination }, &clip).is_err());
        let invalid = CellClipboard { width: 0, ..clip.clone() };
        assert!(session.paste_cells(destination, &invalid).is_err());
        assert_eq!(session.edits(), &before);
    }

    #[test]
    fn copy_between_worksheets_preserves_explicit_sheet_references() {
        let mut session = Session::new(Workbook { sheets: vec![Sheet { name: "Values".into(), cells: vec![] }, Sheet { name: "Other Sheet".into(), cells: vec![] }] });
        let source = Address { sheet: 0, row: 1, column: 1 };
        let target = Address { sheet: 1, row: 3, column: 3 };
        session.set(source, CellInput::Formula("A1+Values!$B2+'Other Sheet'!C$1".into())).unwrap();
        let clip = session.copy_cells(source, source).unwrap();
        session.paste_cells(target, &clip).unwrap();
        assert_eq!(session.input(target), Some(CellInput::Formula("C3+Values!$B4+'Other Sheet'!E$1".into())));
        assert!(session.undo());
        assert_eq!(session.input(target), Some(CellInput::Clear));
    }
}
