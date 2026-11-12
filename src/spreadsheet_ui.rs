//! Workbook preview and guarded cell editing.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use gpui::*;
use gpui::prelude::FluentBuilder;
use crate::app::Workspace;
use crate::input::{TextField, TextFieldEvent};
use crate::spreadsheet_save::{SaveError, SpreadsheetDocument};
use crate::theme::Theme;
use crate::trash::{close_button, dialog};
use crate::ui::{TipExt, TipSide, ToastKind};

#[derive(serde::Serialize, serde::Deserialize)]
struct SpreadsheetClipboard {
    source_id: String,
    cells: fdl_core::spreadsheet_edit::CellClipboard,
}

#[derive(Clone, Copy)]
enum SheetNameAction { Add, Rename(usize) }

enum SpreadsheetBatch {
    Replace { query: String, replacement: String, mode: fdl_core::spreadsheet_edit::ReplaceMode, options: fdl_core::spreadsheet_edit::ReplaceOptions },
    Arithmetic { a: fdl_core::spreadsheet_edit::Address, b: fdl_core::spreadsheet_edit::Address, operation: fdl_core::spreadsheet_edit::Arithmetic, operand: f64, include_formulas: bool },
    Format { a: fdl_core::spreadsheet_edit::Address, b: fdl_core::spreadsheet_edit::Address, format: fdl_core::spreadsheet_xlsx::NumberFormat },
}

pub(crate) struct SpreadsheetPreview {
    id: uuid::Uuid,
    path: PathBuf,
    document: Option<SpreadsheetDocument>,
    field: Entity<TextField>,
    address_field: Entity<TextField>,
    search_field: Entity<TextField>,
    replace_field: Entity<TextField>,
    replace_options: fdl_core::spreadsheet_edit::ReplaceOptions,
    arithmetic_field: Entity<TextField>,
    arithmetic_formulas: bool,
    sheet_name_field: Entity<TextField>,
    sheet_name_action: Option<SheetNameAction>,
    confirm_sheet_delete: Option<usize>,
    _subs: Vec<Subscription>,
    field_base: String,
    saving: bool,
    confirm_discard: bool,
    change_generation: u64,
    calculation_generation: u64,
    calculating: bool,
    calculation_failed: bool,
    conditional_unavailable: bool,
    formula_results: BTreeMap<fdl_core::spreadsheet_edit::Address, fdl_core::spreadsheet_calc::FormulaResult>,
    formatted_values: BTreeMap<fdl_core::spreadsheet_edit::Address, String>,
    percentage_styles: fdl_core::spreadsheet_calc::PercentageStyles,
    appearances: fdl_core::spreadsheet_calc::AppearanceStyles,
    sheet: usize,
    row_page: usize,
    col_page: usize,
    selected: Option<(u32, u32)>,
    selection_end: Option<(u32, u32)>,
    pending_selection: Option<(u32, u32)>,
}

pub(crate) fn is_spreadsheet(path: &Path) -> bool {
    fdl_core::extensions::builtin_editor(path) == Some(fdl_core::extensions::BuiltinEditor::Spreadsheet)
}

fn column_name(mut col: u32) -> String {
    let mut name = String::new();
    loop {
        name.insert(0, (b'A' + (col % 26) as u8) as char);
        if col < 26 { break; }
        col = col / 26 - 1;
    }
    name
}

fn parse_address(input: &str) -> Option<(u32, u32)> {
    let input = input.trim().strip_prefix('$').unwrap_or(input.trim());
    let letters = input.bytes().take_while(u8::is_ascii_alphabetic).count();
    if !(1..=3).contains(&letters) { return None; }
    let (column, row) = input.split_at(letters);
    let row = row.strip_prefix('$').unwrap_or(row);
    if row.is_empty() || row.len() > 7 || !row.bytes().all(|b| b.is_ascii_digit()) { return None; }
    let row = row.parse::<u32>().ok()?;
    let column = column.bytes().fold(0u32, |n, b| n * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1));
    if !(1..=1_048_576).contains(&row) || !(1..=16_384).contains(&column) { return None; }
    Some((row - 1, column - 1))
}

fn input_text(input: &fdl_core::spreadsheet_edit::CellInput) -> String {
    use fdl_core::spreadsheet_edit::CellInput;
    match input {
        CellInput::Clear => String::new(),
        CellInput::Text(s) => format!("'{s}"),
        CellInput::Number(n) => n.to_string(),
        CellInput::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        CellInput::Formula(f) => format!("={f}"),
    }
}

fn formula_text(result: &fdl_core::spreadsheet_calc::FormulaResult) -> String {
    use fdl_core::spreadsheet_calc::FormulaResult;
    match result {
        FormulaResult::Empty => String::new(),
        FormulaResult::Number(n) => n.to_string(),
        FormulaResult::Text(s) | FormulaResult::Error(s) => s.clone(),
        FormulaResult::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
    }
}

fn cell_display_text(cell: &fdl_core::spreadsheet::Cell, address: fdl_core::spreadsheet_edit::Address, formulas: &BTreeMap<fdl_core::spreadsheet_edit::Address, fdl_core::spreadsheet_calc::FormulaResult>, formatted: &BTreeMap<fdl_core::spreadsheet_edit::Address, String>) -> String {
    if cell.formula.is_some() {
        formulas.get(&address).map(|value| formatted.get(&address).cloned().unwrap_or_else(|| formula_text(value))).unwrap_or_else(|| "—".to_string())
    } else { formatted.get(&address).cloned().unwrap_or_else(|| fdl_core::spreadsheet_edit::fallback_cell_text(cell)) }
}

fn sheet_change_error(error: &SaveError) -> String {
    match error {
        SaveError::Changed => crate::settings::tr("文件已被其他程序修改，未更改工作表", "The file changed externally; no worksheet was changed").to_string(),
        SaveError::Unsupported(error) => {
            let reason = error.to_string();
            match reason.as_str() {
                "invalid worksheet name" => crate::settings::tr("工作表名称无效，请检查长度及特殊字符", "Invalid worksheet name; check its length and special characters").to_string(),
                "worksheet name already exists" => crate::settings::tr("工作表名称已存在", "A worksheet with that name already exists").to_string(),
                "workbook structure is protected" => crate::settings::tr("工作簿结构受保护，无法更改工作表", "Workbook structure is protected").to_string(),
                "cannot delete the last worksheet" | "cannot delete the last visible worksheet" => crate::settings::tr("必须保留至少一个可见工作表", "At least one visible worksheet must remain").to_string(),
                "undo history contains a possible worksheet reference" => crate::settings::tr("撤销历史可能引用此工作表，请保存并重新打开后重试", "Undo history may reference this worksheet; save and reopen the workbook to retry").to_string(),
                "worksheet name is referenced elsewhere; safe rename is unavailable" | "worksheet is referenced elsewhere; safe deletion is unavailable" | "worksheet name appears in a package path" | "worksheet name or part appears in a package path" | "worksheet name has unsupported escape-like text" | "workbook contains unsupported extensions" | "workbook contains unsupported binary parts" | "workbook contains unsupported custom XML" | "workbook contains unsupported sheet-indexed metadata" | "workbook contains unsupported escaped text" | "workbook contains unsupported split XML content" | "worksheet has dependent relationships" | "worksheet has dependent content" | "workbook has unsupported sheet-indexed metadata" | "workbook calculation chain is not supported for sheet deletion" => crate::settings::tr("工作簿可能包含受影响的引用或扩展，暂不能安全更改", "The workbook may contain affected references or extensions; it cannot yet be changed safely").to_string(),
                _ => crate::localized_format!("无法更改工作表：{}", "Cannot change worksheet: {}", crate::settings::diagnostic(&reason)),
            }
        }
        SaveError::Io(error) => crate::localized_format!("无法更改工作表：{}", "Cannot change worksheet: {}", crate::settings::diagnostic(&error.to_string())),
    }
}

impl Workspace {
    fn invalidate_spreadsheet_calculation(&mut self) {
        if let Some(p) = &mut self.spreadsheet_preview {
            p.calculation_generation = p.calculation_generation.wrapping_add(1);
            p.formula_results.clear();
            p.formatted_values.clear();
            p.calculation_failed = false;
        }
    }

    fn calculate_spreadsheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.spreadsheet_preview else { return };
        let Some(document) = &p.document else { return };
        if p.calculating || p.saving || document.session.is_dirty() || p.field.read(cx).text != p.field_base { return; }
        let snapshot = document.calculation_snapshot();
        let task_snapshot = snapshot.clone();
        let (id, generation) = (p.id, p.calculation_generation);
        p.calculating = true;
        cx.notify();
        let task = cx.background_executor().spawn(async move { fdl_core::spreadsheet_calc::evaluate(&task_snapshot) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                let Some(p) = &mut w.spreadsheet_preview else { return };
                if p.id != id { return; }
                p.calculating = false;
                let stale = p.calculation_generation != generation || p.saving || p.document.as_ref().is_none_or(|d| d.session.is_dirty()) || p.field.read(cx).text != p.field_base;
                let same_snapshot = !p.saving && p.document.as_ref().is_some_and(|d| std::sync::Arc::ptr_eq(&snapshot, &d.calculation_snapshot()));
                let mut retry_input = false;
                if !stale {
                    match result {
                        Ok(values) => { p.formula_results = values.formulas; p.formatted_values = values.formatted; p.percentage_styles = values.percentage_styles; p.appearances = values.appearances; p.calculation_failed = values.calculation_unavailable; p.conditional_unavailable = values.conditional_unavailable; }
                        Err(_) => { p.formula_results.clear(); p.formatted_values.clear(); p.percentage_styles.clear(); p.appearances.clear(); p.calculation_failed = true; p.conditional_unavailable = false; }
                    }
                    if let (Some(document), Some((row, column))) = (&p.document, p.selected) {
                        let address = fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row, column };
                        if let Some(value) = document.session.display_input(address) {
                            let text = document.session.date_input_text_with_styles(address, &value, &p.percentage_styles).unwrap_or_else(|| input_text(&value));
                            p.field_base = text.clone();
                            p.field.update(cx, |field, cx| field.set_table_text(&text, cx));
                        }
                    }
                } else if same_snapshot && let Ok(values) = result {
                    p.percentage_styles = values.percentage_styles;
                    retry_input = p.field.read(cx).text != p.field_base;
                }
                cx.notify();
                if retry_input { w.schedule_spreadsheet_edit(window, cx); }
                else if stale { w.calculate_spreadsheet(window, cx); }
            });
        }).detach();
    }

    pub(crate) fn spreadsheet_has_pending(&self, cx: &App) -> bool {
        self.spreadsheet_preview.as_ref().is_some_and(|p| p.saving || p.document.as_ref().is_some_and(|d| d.session.is_dirty()) || p.field.read(cx).text != p.field_base)
    }

    fn schedule_spreadsheet_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.invalidate_spreadsheet_calculation();
        let Some(p) = &mut self.spreadsheet_preview else { return };
        p.change_generation = p.change_generation.wrapping_add(1);
        let (id, generation) = (p.id, p.change_generation);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(800)).await;
            let _ = this.update_in(cx, |w, window, cx| {
                if w.spreadsheet_preview.as_ref().is_some_and(|p| p.id == id && p.change_generation == generation && !p.saving) {
                    w.apply_spreadsheet_edit(window, cx);
                }
            });
        }).detach();
    }

    fn select_spreadsheet_cell(&mut self, row: u32, column: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.spreadsheet_preview else { return };
        if p.saving || p.field.read(cx).text != p.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请等待当前单元格保存", "Wait for the current cell to save"), cx);
            return;
        }
        let Some(document) = &p.document else { return };
        let address = fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row, column };
        let Some(input) = document.session.display_input(address) else {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("此单元格类型暂不能安全编辑", "This cell type cannot yet be edited safely"), cx);
            return;
        };
        let text = document.session.date_input_text_with_styles(address, &input, &p.percentage_styles).unwrap_or_else(|| input_text(&input));
        p.selected = Some((row, column));
        p.selection_end = None;
        p.pending_selection = None;
        p.row_page = row as usize / 30;
        p.col_page = column as usize / 8;
        p.field_base = text.clone();
        p.change_generation = p.change_generation.wrapping_add(1);
        p.field.update(cx, |field, cx| field.set_table_text(&text, cx));
        p.address_field.update(cx, |field, cx| field.set_text(&format!("{}{}", column_name(column), row + 1), cx));
        if document.session.is_hidden_formula(address) { window.focus(&p.address_field.focus_handle(cx)); }
        else { window.focus(&p.field.focus_handle(cx)); }
        cx.notify();
    }

    fn select_spreadsheet_range(&mut self, row: u32, column: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.spreadsheet_preview else { return };
        if p.saving || p.field.read(cx).text != p.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请等待当前单元格保存", "Wait for the current cell to save"), cx);
            return;
        }
        if p.selected.is_none() { self.select_spreadsheet_cell(row, column, window, cx); return; }
        p.selection_end = Some((row, column));
        cx.notify();
    }

    fn copy_spreadsheet_values(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &self.spreadsheet_preview else { return };
        if p.saving || p.field.read(cx).text != p.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请等待当前单元格保存", "Wait for the current cell to save"), cx);
            return;
        }
        let (Some(document), Some((row, column))) = (&p.document, p.selected) else { return };
        let (end_row, end_column) = p.selection_end.unwrap_or((row, column));
        let a = fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row, column };
        let b = fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row: end_row, column: end_column };
        match document.session.copy_values_tsv(a, b, &p.formula_results) {
            Ok(text) => { cx.write_to_clipboard(ClipboardItem::new_string(text)); self.show_toast_kind(ToastKind::Success, crate::settings::tr("已复制单元格值", "Cell values copied"), cx); }
            Err(_) => self.show_toast_kind(ToastKind::Error, crate::settings::tr("无法复制：区域过大或公式结果尚不可用", "Cannot copy: selection is too large or formula values are unavailable"), cx),
        }
    }

    fn copy_spreadsheet_cells(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &self.spreadsheet_preview else { return };
        if p.saving || p.field.read(cx).text != p.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请等待当前单元格保存", "Wait for the current cell to save"), cx);
            return;
        }
        let (Some(document), Some((row, column))) = (&p.document, p.selected) else { return };
        let (end_row, end_column) = p.selection_end.unwrap_or((row, column));
        let a = fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row, column };
        let b = fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row: end_row, column: end_column };
        let result = document.session.copy_cells(a, b).and_then(|cells| {
            let text = cells.plain_tsv()?;
            let payload = serde_json::to_string(&SpreadsheetClipboard { source_id: p.id.to_string(), cells })?;
            anyhow::ensure!(payload.len() <= 16 * 1024 * 1024, "clipboard metadata is too large");
            Ok((text, format!("{}{payload}", crate::input::SPREADSHEET_CLIPBOARD_PREFIX)))
        });
        match result {
            Ok((text, metadata)) => { cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(text, metadata)); self.show_toast_kind(ToastKind::Success, crate::settings::tr("已复制单元格", "Cells copied"), cx); }
            Err(_) => self.show_toast_kind(ToastKind::Error, crate::settings::tr("无法复制：区域过大或包含暂不支持的单元格", "Cannot copy: selection is too large or contains unsupported cells"), cx),
        }
    }

    fn fill_spreadsheet_range(&mut self, clear: bool, window: &mut Window, cx: &mut Context<Self>) {
        use fdl_core::spreadsheet_edit::{Address, CellInput};
        let Some(p) = &mut self.spreadsheet_preview else { return };
        if p.saving || p.field.read(cx).text != p.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请等待当前单元格保存", "Wait for the current cell to save"), cx);
            return;
        }
        let (Some(document), Some((row, column))) = (&mut p.document, p.selected) else { return };
        let (end_row, end_column) = p.selection_end.unwrap_or((row, column));
        let start = Address { sheet: p.sheet, row, column };
        let end = Address { sheet: p.sheet, row: end_row, column: end_column };
        let result = if clear { document.session.fill_range(start, end, CellInput::Clear) } else {
            match document.session.input(start) {
                Some(CellInput::Formula(_)) => document.session.fill_formula(start, end),
                Some(value) => document.session.fill_range(start, end, value),
                None => return,
            }
        };
        if result.is_err() {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("无法修改区域：超出范围或包含暂不支持的单元格、公式", "Cannot edit range: out-of-range or unsupported cells or formula"), cx);
            return;
        }
        if let Some(value) = document.session.display_input(start) {
            let text = document.session.date_input_text_with_styles(start, &value, &p.percentage_styles).unwrap_or_else(|| input_text(&value));
            p.field_base = text.clone();
            p.change_generation = p.change_generation.wrapping_add(1);
            p.field.update(cx, |field, cx| field.set_table_text(&text, cx));
        }
        self.save_spreadsheet(window, cx);
    }

    fn jump_spreadsheet_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = &self.spreadsheet_preview else { return };
        let address = parse_address(&preview.address_field.read(cx).text);
        if let Some((row, column)) = address { self.select_spreadsheet_cell(row, column, window, cx); }
        else { self.show_toast_kind(ToastKind::Error, crate::settings::tr("请输入有效的单元格地址，例如 A1", "Enter a valid cell address, such as A1"), cx); }
    }

    fn find_spreadsheet_cell(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &self.spreadsheet_preview else { return };
        if p.saving || p.field.read(cx).text != p.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请等待当前单元格保存", "Wait for the current cell to save"), cx);
            return;
        }
        let Some(document) = &p.document else { return };
        let query = p.search_field.read(cx).text.clone();
        if query.is_empty() { return; }
        let found = document.session.find_cell(p.sheet, &query, p.selected, reverse, &p.formula_results, &p.formatted_values);
        let focus = p.search_field.focus_handle(cx);
        if let Some(found) = found {
            self.select_spreadsheet_cell(found.row, found.column, window, cx);
            window.focus(&focus);
        } else { self.show_toast_kind(ToastKind::Info, crate::settings::tr("当前工作表没有匹配项", "No matches in this worksheet"), cx); }
    }

    fn replace_spreadsheet_all(&mut self, mode: fdl_core::spreadsheet_edit::ReplaceMode, window: &mut Window, cx: &mut Context<Self>) {
        if self.spreadsheet_has_pending(cx) {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请先保存当前单元格", "Save the current cell first"), cx);
            return;
        }
        let Some(p) = &self.spreadsheet_preview else { return };
        let query = p.search_field.read(cx).text.clone();
        let replacement = p.replace_field.read(cx).text.clone();
        if query.is_empty() { self.show_toast_kind(ToastKind::Info, crate::settings::tr("请输入要查找的内容", "Enter text to find"), cx); return; }
        let batch = SpreadsheetBatch::Replace { query, replacement, mode, options: p.replace_options };
        self.run_spreadsheet_batch(batch, window, cx);
    }

    fn arithmetic_spreadsheet_range(&mut self, operation: fdl_core::spreadsheet_edit::Arithmetic, window: &mut Window, cx: &mut Context<Self>) {
        use fdl_core::spreadsheet_edit::{Address, CellInput};
        if self.spreadsheet_has_pending(cx) {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请先保存当前单元格", "Save the current cell first"), cx);
            return;
        }
        let Some(p) = &self.spreadsheet_preview else { return };
        let Some((row, column)) = p.selected else { return };
        let (end_row, end_column) = p.selection_end.unwrap_or((row, column));
        let text = p.arithmetic_field.read(cx).text.clone();
        let Ok(CellInput::Number(operand)) = fdl_core::spreadsheet_edit::parse_input_with_format(&text, Some(true)) else {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("请输入有效的数值或百分比", "Enter a valid number or percentage"), cx);
            return;
        };
        if operation == fdl_core::spreadsheet_edit::Arithmetic::Divide && operand == 0.0 {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("除数不能为零", "Cannot divide by zero"), cx);
            return;
        }
        let a = Address { sheet: p.sheet, row, column };
        let b = Address { sheet: p.sheet, row: end_row, column: end_column };
        let batch = SpreadsheetBatch::Arithmetic { a, b, operation, operand, include_formulas: p.arithmetic_formulas };
        self.run_spreadsheet_batch(batch, window, cx);
    }

    fn run_spreadsheet_batch(&mut self, batch: SpreadsheetBatch, window: &mut Window, cx: &mut Context<Self>) {
        let is_replace = matches!(batch, SpreadsheetBatch::Replace { .. });
        let is_format = matches!(batch, SpreadsheetBatch::Format { .. });
        self.invalidate_spreadsheet_calculation();
        let Some(p) = &mut self.spreadsheet_preview else { return };
        let Some(mut document) = p.document.take() else { return };
        let (id, sheet, styles) = (p.id, p.sheet, p.percentage_styles.clone());
        p.saving = true;
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            let result = match batch {
                SpreadsheetBatch::Replace { query, replacement, mode, options } => document.replace_all_with_options(sheet, &query, &replacement, mode, &styles, options),
                SpreadsheetBatch::Arithmetic { a, b, operation, operand, include_formulas } => document.arithmetic_range(a, b, operation, operand, include_formulas, &styles),
                SpreadsheetBatch::Format { a, b, format } => document.format_number_range(a, b, format),
            };
            (document, result)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (document, result) = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                let Some(p) = &mut w.spreadsheet_preview else { return };
                if p.id != id { return; }
                p.saving = false;
                if is_format && result.is_ok() { p.percentage_styles.clear(); }
                if let Some((row, column)) = p.selected {
                    let address = fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row, column };
                    if let Some(value) = document.session.display_input(address) {
                        let text = document.session.date_input_text_with_styles(address, &value, &p.percentage_styles).unwrap_or_else(|| input_text(&value));
                        p.field_base = text.clone();
                        p.field.update(cx, |field, cx| field.set_table_text(&text, cx));
                    }
                }
                p.document = Some(document);
                if is_replace { window.focus(&p.replace_field.focus_handle(cx)); }
                else if !is_format { window.focus(&p.arithmetic_field.focus_handle(cx)); }
                match result {
                    Ok(count) => {
                        let message = crate::localized_format!("已更新 {} 个单元格", "Updated {} cells", count);
                        w.show_toast_kind(ToastKind::Success, &message, cx);
                        w.calculate_spreadsheet(window, cx);
                    }
                    Err(error) => {
                        let message = match error {
                            SaveError::Changed if is_format => crate::settings::tr("文件已被其他程序修改；未应用格式，也未覆盖文件", "The file changed externally; formatting was not applied and the file was not overwritten").to_string(),
                            SaveError::Changed => crate::settings::tr("文件已被其他程序修改；更改保留在窗口中，未覆盖文件", "The file changed externally; edits remain in this window and were not saved").to_string(),
                            SaveError::Unsupported(e) => {
                                let reason = e.to_string();
                                match reason.as_str() {
                                    "replacement is not a valid value of the same type" => crate::settings::tr("替换后的内容不是同类型的有效数值或布尔值；未应用替换", "Replacement must be a valid value of the same type; no changes were applied").to_string(),
                                    "replacement is not a valid date" => crate::settings::tr("替换后的日期无效；未应用替换", "Replacement date is invalid; no changes were applied").to_string(),
                                    "replacement must retain the formula's leading equals sign" => crate::settings::tr("公式必须保留开头的等号；未应用替换", "A formula must retain its leading equals sign; no changes were applied").to_string(),
                                    "replacement is too large" => crate::settings::tr("替换范围或内容过大；未应用替换", "Replacement is too large; no changes were applied").to_string(),
                                    "cell format is unavailable for arithmetic" => crate::settings::tr("无法识别单元格格式，请等待试算完成；未应用运算", "Cell formatting is unavailable; wait for calculation. No arithmetic was applied").to_string(),
                                    "arithmetic result is outside the supported range" => crate::settings::tr("运算结果超出数值范围；未应用运算", "An arithmetic result is out of range; no changes were applied").to_string(),
                                    "worksheet contains locked cells" => crate::settings::tr("工作表受保护，选区包含锁定单元格；未应用更改", "The protected worksheet contains locked cells; no changes were applied").to_string(),
                                    "worksheet formatting is protected" => crate::settings::tr("工作表受保护，不允许修改格式", "Worksheet protection does not allow formatting").to_string(),
                                    _ => crate::localized_format!("无法安全应用更改：{}", "Cannot safely apply changes: {}", crate::settings::diagnostic(&reason)),
                                }
                            }
                            SaveError::Io(e) => crate::localized_format!("保存更改失败：{}", "Could not save changes: {}", crate::settings::diagnostic(&e.to_string())),
                        };
                        w.show_toast_kind(ToastKind::Error, &message, cx);
                        w.calculate_spreadsheet(window, cx);
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    fn format_spreadsheet_range(&mut self, format: fdl_core::spreadsheet_xlsx::NumberFormat, window: &mut Window, cx: &mut Context<Self>) {
        use fdl_core::spreadsheet_edit::Address;
        if self.spreadsheet_has_pending(cx) {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请先保存当前单元格", "Save the current cell first"), cx);
            return;
        }
        let Some(p) = &self.spreadsheet_preview else { return };
        let Some((row, column)) = p.selected else { return };
        let (end_row, end_column) = p.selection_end.unwrap_or((row, column));
        let a = Address { sheet: p.sheet, row, column };
        let b = Address { sheet: p.sheet, row: end_row, column: end_column };
        self.run_spreadsheet_batch(SpreadsheetBatch::Format { a, b, format }, window, cx);
    }

    fn toggle_spreadsheet_replace_case(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.spreadsheet_preview { p.replace_options.case_sensitive = !p.replace_options.case_sensitive; }
        cx.notify();
    }

    fn toggle_spreadsheet_replace_whole(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.spreadsheet_preview { p.replace_options.whole_cell = !p.replace_options.whole_cell; }
        cx.notify();
    }

    fn toggle_spreadsheet_arithmetic_formulas(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.spreadsheet_preview { p.arithmetic_formulas = !p.arithmetic_formulas; }
        cx.notify();
    }

    pub(crate) fn focus_spreadsheet_find(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(p) = &self.spreadsheet_preview else { return false };
        if p.document.is_some() { window.focus(&p.search_field.focus_handle(cx)); }
        true
    }

    fn select_spreadsheet_sheet(&mut self, sheet: usize, cx: &mut Context<Self>) {
        let Some(p) = &mut self.spreadsheet_preview else { return };
        if p.saving || p.field.read(cx).text != p.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请等待当前单元格保存", "Wait for the current cell to save"), cx);
            return;
        }
        if !p.document.as_ref().is_some_and(|document| sheet < document.session.workbook.sheets.len()) { return; }
        p.sheet = sheet;
        p.sheet_name_action = None;
        p.confirm_sheet_delete = None;
        p.row_page = 0;
        p.col_page = 0;
        p.selected = None;
        p.selection_end = None;
        p.pending_selection = None;
        p.change_generation = p.change_generation.wrapping_add(1);
        p.field_base.clear();
        p.field.update(cx, |field, cx| field.set_table_text("", cx));
        p.address_field.update(cx, |field, cx| field.set_text("", cx));
        cx.notify();
    }

    fn prompt_spreadsheet_sheet_name(&mut self, action: SheetNameAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.spreadsheet_has_pending(cx) {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请先保存当前工作簿", "Save the current workbook first"), cx);
            return;
        }
        let Some(p) = &mut self.spreadsheet_preview else { return };
        let Some(document) = &p.document else { return };
        let initial = match action {
            SheetNameAction::Add => String::new(),
            SheetNameAction::Rename(index) => match document.session.workbook.sheets.get(index) { Some(sheet) => sheet.name.clone(), None => return },
        };
        p.sheet_name_action = Some(action);
        p.confirm_sheet_delete = None;
        p.sheet_name_field.update(cx, |field, cx| { field.set_text(&initial, cx); field.select_all_text(cx); });
        window.focus(&p.sheet_name_field.focus_handle(cx));
        cx.notify();
    }

    fn commit_spreadsheet_sheet_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.spreadsheet_has_pending(cx) {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请先保存当前工作簿", "Save the current workbook first"), cx);
            return;
        }
        let Some(p) = &self.spreadsheet_preview else { return };
        let Some(action) = p.sheet_name_action else { return };
        let name = p.sheet_name_field.read(cx).text.trim().to_string();
        if name.is_empty() { return; }
        let renamed = match action { SheetNameAction::Rename(index) => p.document.as_ref().and_then(|d| d.session.workbook.sheets.get(index)).is_some_and(|s| s.name != name), SheetNameAction::Add => false };
        self.invalidate_spreadsheet_calculation();
        let Some(p) = &mut self.spreadsheet_preview else { return };
        let Some(mut document) = p.document.take() else { return };
        let id = p.id;
        p.saving = true;
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            let result = match action { SheetNameAction::Add => document.add_worksheet(&name), SheetNameAction::Rename(index) => document.rename_worksheet(index, &name) };
            (document, result)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (document, result) = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                let Some(p) = &mut w.spreadsheet_preview else { return };
                if p.id != id { return; }
                p.saving = false;
                let last = document.session.workbook.sheets.len().saturating_sub(1);
                p.document = Some(document);
                if let Err(error) = result {
                    let message = sheet_change_error(&error);
                    w.show_toast_kind(ToastKind::Error, &message, cx);
                    if let Some(p) = &w.spreadsheet_preview { window.focus(&p.sheet_name_field.focus_handle(cx)); }
                    w.calculate_spreadsheet(window, cx);
                } else {
                    if let Some(p) = &mut w.spreadsheet_preview {
                        p.sheet_name_action = None;
                        if renamed { p.id = uuid::Uuid::new_v4(); p.calculating = false; }
                    }
                    match action {
                        SheetNameAction::Add => { w.select_spreadsheet_sheet(last, cx); w.select_spreadsheet_cell(0, 0, window, cx); }
                        SheetNameAction::Rename(_) => { if let Some(p) = &w.spreadsheet_preview && let Some((row, column)) = p.selected {
                            let hidden = p.document.as_ref().is_some_and(|d| d.session.is_hidden_formula(fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row, column }));
                            window.focus(&if hidden { p.address_field.focus_handle(cx) } else { p.field.focus_handle(cx) });
                        } }
                    }
                    w.calculate_spreadsheet(window, cx);
                }
                cx.notify();
            });
        }).detach();
    }

    fn prompt_delete_spreadsheet_sheet(&mut self, cx: &mut Context<Self>) {
        if self.spreadsheet_has_pending(cx) {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请先保存当前工作簿", "Save the current workbook first"), cx);
            return;
        }
        let Some(p) = &mut self.spreadsheet_preview else { return };
        let Some(document) = &p.document else { return };
        if document.session.workbook.sheets.len() <= 1 {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("必须保留至少一个可见工作表", "At least one visible worksheet must remain"), cx);
            return;
        }
        p.sheet_name_action = None;
        p.confirm_sheet_delete = Some(p.sheet);
        cx.notify();
    }

    fn delete_spreadsheet_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.spreadsheet_has_pending(cx) {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请先保存当前工作簿", "Save the current workbook first"), cx);
            return;
        }
        let Some(p) = &self.spreadsheet_preview else { return };
        let Some(index) = p.confirm_sheet_delete.filter(|index| *index == p.sheet) else { return };
        self.invalidate_spreadsheet_calculation();
        let Some(p) = &mut self.spreadsheet_preview else { return };
        let Some(mut document) = p.document.take() else { return };
        let id = p.id;
        p.saving = true;
        cx.notify();
        let task = cx.background_executor().spawn(async move { let result = document.delete_worksheet(index); (document, result) });
        cx.spawn_in(window, async move |this, cx| {
            let (document, result) = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                let Some(p) = &mut w.spreadsheet_preview else { return };
                if p.id != id { return; }
                p.saving = false;
                let next = index.min(document.session.workbook.sheets.len().saturating_sub(1));
                p.document = Some(document);
                if let Err(error) = result {
                    w.show_toast_kind(ToastKind::Error, &sheet_change_error(&error), cx);
                    w.calculate_spreadsheet(window, cx);
                } else {
                    if let Some(p) = &mut w.spreadsheet_preview { p.id = uuid::Uuid::new_v4(); p.calculating = false; p.confirm_sheet_delete = None; p.percentage_styles.clear(); p.appearances.clear(); }
                    w.select_spreadsheet_sheet(next, cx);
                    w.select_spreadsheet_cell(0, 0, window, cx);
                    w.calculate_spreadsheet(window, cx);
                }
                cx.notify();
            });
        }).detach();
    }

    fn apply_spreadsheet_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.spreadsheet_preview else { return };
        if p.saving { return; }
        let Some((row, column)) = p.selected else { return };
        let text = p.field.read(cx).text.clone();
        let Some(document) = &mut p.document else { return };
        if text != p.field_base {
            let address = fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row, column };
            let value = match document.session.parse_input_at_with_styles(address, &text, &p.percentage_styles) {
                Ok(value) => value,
                Err(_) => {
                    self.show_toast_kind(ToastKind::Error, crate::settings::tr("日期或百分比输入无效；文本可用单引号开头", "Invalid date or percentage; prefix text with an apostrophe"), cx);
                    return;
                }
            };
            if document.session.set(address, value).is_err() {
                self.show_toast_kind(ToastKind::Error, crate::settings::tr("单元格内容无效或超出限制", "Invalid cell content or limit exceeded"), cx);
                return;
            }
            p.field_base = text;
            p.field.update(cx, |field, _| field.mark_table_committed());
        }
        self.save_spreadsheet(window, cx);
    }

    fn save_spreadsheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_spreadsheet(None, window, cx);
    }

    fn commit_spreadsheet(&mut self, destination: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.spreadsheet_preview.as_ref().is_some_and(|p| !p.saving && p.document.as_ref().is_some_and(|document| document.session.is_dirty())) { cx.notify(); return; }
        if self.spreadsheet_preview.as_ref().is_some_and(|p| p.field.read(cx).text != p.field_base) {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请先完成当前单元格输入", "Finish the current cell input first"), cx);
            return;
        }
        self.invalidate_spreadsheet_calculation();
        let Some(p) = &mut self.spreadsheet_preview else { return };
        let Some(mut document) = p.document.take() else { return };
        p.saving = true;
        let id = p.id;
        cx.notify();
        let task = cx.background_executor().spawn(async move { let result = match destination { Some(path) => document.save_copy(&path), None => document.save() }; (document, result) });
        cx.spawn_in(window, async move |this, cx| {
            let (document, result) = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                let Some(p) = &mut w.spreadsheet_preview else { return };
                if p.id != id { return; }
                p.saving = false;
                p.path = document.path.clone();
                p.document = Some(document);
                let next = if result.is_ok() { p.pending_selection.take() } else { None };
                if let Some((row, column)) = p.selected {
                    let hidden = p.document.as_ref().is_some_and(|d| d.session.is_hidden_formula(fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row, column }));
                    if hidden { p.field_base.clear(); p.field.update(cx, |field, cx| field.set_table_text("", cx)); }
                    window.focus(&if hidden { p.address_field.focus_handle(cx) } else { p.field.focus_handle(cx) });
                }
                if let Err(error) = result {
                    let message = match error {
                        SaveError::Changed => crate::settings::tr("文件已被其他程序修改，未覆盖；修改仍保留在此窗口", "The file changed externally and was not overwritten; your edits remain here").to_string(),
                        SaveError::Unsupported(e) if e.to_string() == "worksheet contains locked cells" => crate::settings::tr("工作表受保护，锁定单元格不能保存；可撤销该修改", "The worksheet is protected and the locked cell cannot be saved; undo the edit").to_string(),
                        SaveError::Unsupported(e) => crate::localized_format!("暂不能安全保存：{}", "Cannot safely save: {}", crate::settings::diagnostic(&e.to_string())),
                        SaveError::Io(e) => crate::localized_format!("保存失败：{}", "Save failed: {}", crate::settings::diagnostic(&e.to_string())),
                    };
                    w.show_toast_kind(ToastKind::Error, &message, cx);
                } else {
                    if let Some(p) = &mut w.spreadsheet_preview { p.confirm_discard = false; }
                    if let Some((row, column)) = next { w.select_spreadsheet_cell(row, column, window, cx); }
                    w.calculate_spreadsheet(window, cx);
                }
                cx.notify();
            });
        }).detach();
    }

    fn save_spreadsheet_copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = &self.spreadsheet_preview else { return };
        if preview.saving || preview.document.as_ref().is_none_or(|d| !d.session.is_dirty()) { return; }
        if preview.field.read(cx).text != preview.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请先完成当前单元格输入", "Finish the current cell input first"), cx);
            return;
        }
        let id = preview.id;
        let directory = preview.path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        let stem = preview.path.file_stem().unwrap_or_default().to_string_lossy();
        let extension = preview.path.extension().unwrap_or_default().to_string_lossy().into_owned();
        let suggested = format!("{stem}-copy.{extension}");
        let picked = crate::devtools::take_pick().and_then(|paths| paths.into_iter().next());
        let prompt = picked.is_none().then(|| cx.prompt_for_new_path(&directory, Some(&suggested)));
        cx.spawn_in(window, async move |this, cx| {
            let path = match prompt { Some(prompt) => prompt.await.ok().and_then(Result::ok).flatten(), None => picked };
            let Some(mut path) = path else { return };
            if path.extension().is_none() { path.set_extension(extension); }
            let _ = this.update_in(cx, |w, window, cx| {
                if w.spreadsheet_preview.as_ref().is_some_and(|p| p.id == id) { w.commit_spreadsheet(Some(path), window, cx); }
            });
        }).detach();
    }

    fn spreadsheet_history(&mut self, undo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.spreadsheet_preview else { return };
        if !p.saving && undo && p.field.read(cx).text != p.field_base {
            let text = p.field_base.clone();
            p.change_generation = p.change_generation.wrapping_add(1);
            p.field.update(cx, |field, cx| field.set_table_text(&text, cx));
            cx.notify();
            self.calculate_spreadsheet(window, cx);
            return;
        }
        if p.saving || p.field.read(cx).text != p.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请等待当前单元格保存", "Wait for the current cell to save"), cx);
            return;
        }
        let Some(document) = &mut p.document else { return };
        let changed = if undo { document.session.undo() } else { document.session.redo() };
        if !changed { return; }
        if !document.session.style_edits().is_empty() { p.percentage_styles.clear(); }
        if let Some((row, column)) = p.selected {
            let address = fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row, column };
            if let Some(value) = document.session.display_input(address) {
                let text = document.session.date_input_text_with_styles(address, &value, &p.percentage_styles).unwrap_or_else(|| input_text(&value));
                p.field_base = text.clone();
                p.field.update(cx, |field, cx| field.set_table_text(&text, cx));
            }
        }
        self.save_spreadsheet(window, cx);
    }

    fn navigate_spreadsheet(&mut self, dr: i32, dc: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.spreadsheet_preview else { return };
        if p.saving { return; }
        let Some((row, column)) = p.selected else { return };
        let next = (row.saturating_add_signed(dr).min(1_048_575), column.saturating_add_signed(dc).min(16_383));
        if next == (row, column) {
            if p.field.read(cx).text != p.field_base { self.apply_spreadsheet_edit(window, cx); }
            return;
        }
        if p.field.read(cx).text != p.field_base {
            p.pending_selection = Some(next);
            self.apply_spreadsheet_edit(window, cx);
            let Some(p) = &mut self.spreadsheet_preview else { return };
            if p.saving { return; }
            if p.field.read(cx).text != p.field_base { p.pending_selection = None; return; }
        }
        self.select_spreadsheet_cell(next.0, next.1, window, cx);
    }

    fn paste_spreadsheet_table(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.spreadsheet_preview else { return };
        if p.saving || p.field.read(cx).text != p.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请等待当前单元格保存", "Wait for the current cell to save"), cx);
            return;
        }
        let Some((row, column)) = p.selected else { return };
        let Some(document) = &mut p.document else { return };
        let start = fdl_core::spreadsheet_edit::Address { sheet: p.sheet, row, column };
        if document.session.paste_tsv_with_formats(start, text, &p.percentage_styles).is_err() {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("无法粘贴区域：内容、格式或单元格暂不支持", "Cannot paste: unsupported content, format or cells"), cx);
            return;
        }
        if let Some(value) = document.session.display_input(start) {
            let text = document.session.date_input_text_with_styles(start, &value, &p.percentage_styles).unwrap_or_else(|| input_text(&value));
            p.field_base = text.clone();
            p.field.update(cx, |field, cx| field.set_table_text(&text, cx));
        }
        self.save_spreadsheet(window, cx);
    }

    fn paste_spreadsheet_cells(&mut self, metadata: &str, window: &mut Window, cx: &mut Context<Self>) {
        use fdl_core::spreadsheet_edit::{Address, CellInput};
        let Some(p) = &mut self.spreadsheet_preview else { return };
        if p.saving || p.field.read(cx).text != p.field_base {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请等待当前单元格保存", "Wait for the current cell to save"), cx);
            return;
        }
        let Some((row, column)) = p.selected else { return };
        let Some(payload) = metadata.strip_prefix(crate::input::SPREADSHEET_CLIPBOARD_PREFIX).filter(|s| s.len() <= 16 * 1024 * 1024).and_then(|s| serde_json::from_str::<SpreadsheetClipboard>(s).ok()) else {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("剪贴板中的表格数据无效", "Invalid spreadsheet clipboard data"), cx);
            return;
        };
        if payload.source_id != p.id.to_string() && payload.cells.cells.iter().any(|cell| matches!(cell, CellInput::Formula(_))) {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("暂不支持跨工作簿粘贴公式；可使用复制值", "Pasting formulas across workbooks is not yet supported; copy values instead"), cx);
            return;
        }
        let Some(document) = &mut p.document else { return };
        let start = Address { sheet: p.sheet, row, column };
        if document.session.paste_cells(start, &payload.cells).is_err() {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("无法粘贴：区域、单元格或公式暂不支持", "Cannot paste: unsupported range, cell or formula"), cx);
            return;
        }
        if let Some(value) = document.session.display_input(start) {
            let text = document.session.date_input_text_with_styles(start, &value, &p.percentage_styles).unwrap_or_else(|| input_text(&value));
            p.field_base = text.clone();
            p.change_generation = p.change_generation.wrapping_add(1);
            p.field.update(cx, |field, cx| field.set_table_text(&text, cx));
        }
        self.save_spreadsheet(window, cx);
    }

    fn close_spreadsheet_preview(&mut self, cx: &mut Context<Self>) {
        if self.spreadsheet_preview.as_ref().is_some_and(|p| p.saving) {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("正在保存工作簿", "Saving workbook"), cx);
        } else if self.spreadsheet_has_pending(cx) {
            if let Some(p) = &mut self.spreadsheet_preview { p.confirm_discard = true; }
            cx.notify();
        } else { self.spreadsheet_preview = None; cx.notify(); }
    }

    pub(crate) fn open_spreadsheet_preview(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.spreadsheet_has_pending(cx) {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("请先保存当前工作簿", "Save the current workbook first"), cx);
            return;
        }
        let id = uuid::Uuid::new_v4();
        let field = cx.new(|cx| {
            let mut field = TextField::new(crate::settings::tr("选择单元格后输入", "Select a cell to edit"), cx);
            field.table_paste = true;
            field.table_copy = true;
            field.tab_events = true;
            field
        });
        let address_field = cx.new(|cx| { let mut f = TextField::new("A1", cx); f.max_len = Some(16); f });
        let search_field = cx.new(|cx| { let mut f = TextField::new(crate::settings::tr("查找当前工作表", "Find in worksheet"), cx); f.max_len = Some(1024); f });
        let replace_field = cx.new(|cx| { let mut f = TextField::new(crate::settings::tr("替换为", "Replace with"), cx); f.max_len = Some(32_767); f });
        let arithmetic_field = cx.new(|cx| { let mut f = TextField::new(crate::settings::tr("数值", "Value"), cx); f.max_len = Some(128); f.set_text("1", cx); f });
        let sheet_name_field = cx.new(|cx| { let mut f = TextField::new(crate::settings::tr("工作表名称", "Worksheet name"), cx); f.max_len = Some(31); f });
        let subs = vec![cx.subscribe_in(&field, window, |w, _, event: &TextFieldEvent, window, cx| match event {
            TextFieldEvent::Submit | TextFieldEvent::Down => w.navigate_spreadsheet(1, 0, window, cx),
            TextFieldEvent::ShiftSubmit | TextFieldEvent::Up => w.navigate_spreadsheet(-1, 0, window, cx),
            TextFieldEvent::Tab => w.navigate_spreadsheet(0, 1, window, cx),
            TextFieldEvent::ShiftTab => w.navigate_spreadsheet(0, -1, window, cx),
            TextFieldEvent::Changed => w.schedule_spreadsheet_edit(window, cx),
            TextFieldEvent::CopyTable => w.copy_spreadsheet_cells(cx),
            TextFieldEvent::PasteTable => {
                let pending = w.spreadsheet_preview.as_ref().map(|p| p.field.update(cx, |field, _| (field.pending_table_paste.take(), field.pending_table_metadata.take())));
                if let Some((text, metadata)) = pending {
                    if let Some(metadata) = metadata { w.paste_spreadsheet_cells(&metadata, window, cx); }
                    else if let Some(text) = text { w.paste_spreadsheet_table(&text, window, cx); }
                }
            }
            _ => {}
        }), cx.subscribe_in(&address_field, window, |w, _, event: &TextFieldEvent, window, cx| {
            if matches!(event, TextFieldEvent::Submit) { w.jump_spreadsheet_address(window, cx); }
        }), cx.subscribe_in(&search_field, window, |w, _, event: &TextFieldEvent, window, cx| {
            match event { TextFieldEvent::Submit => w.find_spreadsheet_cell(false, window, cx), TextFieldEvent::ShiftSubmit => w.find_spreadsheet_cell(true, window, cx), _ => {} }
        }), cx.subscribe_in(&sheet_name_field, window, |w, _, event: &TextFieldEvent, window, cx| {
            match event {
                TextFieldEvent::Submit => w.commit_spreadsheet_sheet_name(window, cx),
                TextFieldEvent::Cancel => { if let Some(p) = &mut w.spreadsheet_preview { p.sheet_name_action = None; } cx.notify(); }
                _ => {}
            }
        })];
        self.plugin_preview = false;
        self.media_preview = None;
        self.spreadsheet_preview = Some(SpreadsheetPreview { id, path: path.clone(), document: None, field, address_field, search_field, replace_field, replace_options: Default::default(), arithmetic_field, arithmetic_formulas: false, sheet_name_field, sheet_name_action: None, confirm_sheet_delete: None, _subs: subs, field_base: String::new(), saving: false, confirm_discard: false, change_generation: 0, calculation_generation: 0, calculating: false, calculation_failed: false, conditional_unavailable: false, formula_results: BTreeMap::new(), formatted_values: BTreeMap::new(), percentage_styles: Default::default(), appearances: Default::default(), sheet: 0, row_page: 0, col_page: 0, selected: None, selection_end: None, pending_selection: None });
        self.close_menus();
        cx.notify();
        let task = cx.background_executor().spawn(async move { SpreadsheetDocument::open(path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                if !w.spreadsheet_preview.as_ref().is_some_and(|p| p.id == id) { return; }
                match result {
                    Ok(document) => { if let Some(p) = &mut w.spreadsheet_preview { p.document = Some(document); } w.calculate_spreadsheet(window, cx); }
                    Err(_) => {
                        w.spreadsheet_preview = None;
                        w.show_toast_kind(ToastKind::Error, crate::settings::tr("无法读取工作簿", "Could not read workbook"), cx);
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    pub(crate) fn render_spreadsheet_preview(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.spreadsheet_preview.as_ref()?;
        let title = state.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let width = (f32::from(window.viewport_size().width) - 32.).clamp(250., 920.);
        let height = (f32::from(window.viewport_size().height) - 32.).clamp(250., 650.);
        let mut content = dialog("spreadsheet-preview").relative().w(px(width)).h(px(height)).flex().flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|w, event: &KeyDownEvent, window, cx| {
                let modifiers = event.keystroke.modifiers;
                let primary = if cfg!(target_os = "macos") { modifiers.platform } else { modifiers.control };
                let key = event.keystroke.key.as_str();
                let cell_focused = w.spreadsheet_preview.as_ref().is_some_and(|p| p.field.read(cx).is_focused(window));
                if primary && !modifiers.alt && cell_focused && (key == "z" || key == "y") {
                    w.spreadsheet_history(key == "z" && !modifiers.shift, window, cx);
                    cx.stop_propagation();
                } else if primary && !modifiers.alt && key == "f" {
                    if let Some(p) = &w.spreadsheet_preview { window.focus(&p.search_field.focus_handle(cx)); }
                    cx.stop_propagation();
                } else if !primary && !modifiers.alt && !modifiers.shift && cell_focused && key == "delete" && w.spreadsheet_preview.as_ref().is_some_and(|p| p.selection_end.is_some() && p.selection_end != p.selected && p.field.read(cx).text == p.field_base) {
                    w.fill_spreadsheet_range(true, window, cx);
                    cx.stop_propagation();
                } else if !primary && !modifiers.alt && key == "escape" {
                    if w.spreadsheet_preview.as_ref().is_some_and(|p| p.field.read(cx).text != p.field_base) { w.spreadsheet_history(true, window, cx); }
                    else { w.close_spreadsheet_preview(cx); }
                    cx.stop_propagation();
                }
            }))
            .child(div().px(px(24.)).pt(px(20.)).pb(px(8.)).text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(close_button("spreadsheet-close").absolute().top(px(18.)).right(px(20.)).on_click(cx.listener(|w, _, _, cx| w.close_spreadsheet_preview(cx))))
            .child(div().px(px(24.)).pb(px(8.)).text_size(px(12.)).text_color(Theme::text_caption())
                .child(if state.calculation_failed { crate::settings::tr("本地格式化或试算不可用；公式结果将在 Excel 中重新计算", "Local formatting or calculation unavailable; formulas recalculate in Excel") }
                    else if state.calculating { crate::settings::tr("正在格式化并试算…", "Formatting and calculating…") }
                    else if state.conditional_unavailable { crate::settings::tr("部分条件格式暂不可显示；原文件未改变", "Some conditional formatting is unavailable; the source is unchanged") }
                    else { crate::settings::tr("自动保存 · Shift+单击选择区域 · 本地试算可能与 Excel 不同", "Auto-save · Shift-click selects a range · calculations may differ from Excel") }));
        if let Some(document) = &state.document {
            let book = &document.session.workbook;
            let mut tabs = div().id("spreadsheet-tabs").px(px(24.)).pb(px(8.)).flex().gap(px(8.)).overflow_x_scroll();
            for (i, sheet) in book.sheets.iter().enumerate() {
                tabs = tabs.child(div().id(("spreadsheet-tab", i)).px(px(10.)).py(px(5.)).rounded(px(4.))
                    .when(i == state.sheet, |d| d.bg(Theme::fill_turninto_active()))
                    .cursor_pointer().child(sheet.name.clone()).on_click(cx.listener(move |w, _, _, cx| w.select_spreadsheet_sheet(i, cx))));
            }
            tabs = tabs.child(div().id("sheet-add").px(px(10.)).py(px(5.)).cursor_pointer().text_color(Theme::primary())
                .on_click(cx.listener(|w, _, window, cx| w.prompt_spreadsheet_sheet_name(SheetNameAction::Add, window, cx))).child(crate::settings::tr("＋ 新建工作表", "+ New worksheet")))
                .child(div().id("sheet-rename").px(px(10.)).py(px(5.)).cursor_pointer().text_color(Theme::primary())
                    .on_click(cx.listener(|w, _, window, cx| { if let Some(sheet) = w.spreadsheet_preview.as_ref().map(|p| p.sheet) { w.prompt_spreadsheet_sheet_name(SheetNameAction::Rename(sheet), window, cx); } }))
                    .child(crate::settings::tr("重命名", "Rename")))
                .child(div().id("sheet-delete").px(px(10.)).py(px(5.)).cursor_pointer().text_color(Theme::danger())
                    .on_click(cx.listener(|w, _, _, cx| w.prompt_delete_spreadsheet_sheet(cx))).child(crate::settings::tr("删除工作表", "Delete worksheet")));
            content = content.child(tabs);
            if let Some(index) = state.confirm_sheet_delete && let Some(sheet) = book.sheets.get(index) {
                content = content.child(div().px(px(24.)).pb(px(10.)).flex().flex_wrap().gap(px(10.)).items_center()
                    .child(div().text_color(Theme::danger()).child(crate::localized_format!("删除「{}」？此操作无法撤销，并会清空单元格撤销历史。", "Delete ‘{}’? This cannot be undone and clears cell undo history.", sheet.name)))
                    .child(crate::ui::secondary_button("sheet-delete-cancel", crate::settings::tr("取消", "Cancel"))
                        .on_click(cx.listener(|w, _, _, cx| { if let Some(p) = &mut w.spreadsheet_preview { p.confirm_sheet_delete = None; } cx.notify(); })))
                    .child(crate::ui::primary_button("sheet-delete-confirm", crate::settings::tr("确认删除", "Delete worksheet")).bg(Theme::danger())
                        .on_click(cx.listener(|w, _, window, cx| w.delete_spreadsheet_sheet(window, cx)) )));
            }
            if let Some(action) = state.sheet_name_action {
                content = content.child(div().px(px(24.)).pb(px(8.)).flex().gap(px(10.)).items_center()
                    .child(div().flex_1().min_w_0().px(px(8.)).py(px(4.)).border_1().border_color(Theme::line_border()).rounded(px(4.)).child(state.sheet_name_field.clone()))
                    .child(div().id("sheet-add-confirm").cursor_pointer().text_color(Theme::primary()).on_click(cx.listener(|w, _, window, cx| w.commit_spreadsheet_sheet_name(window, cx)))
                        .child(match action { SheetNameAction::Add => crate::settings::tr("创建", "Create"), SheetNameAction::Rename(_) => crate::settings::tr("重命名", "Rename") }))
                    .child(div().id("sheet-add-cancel").cursor_pointer().on_click(cx.listener(|w, _, _, cx| { if let Some(p) = &mut w.spreadsheet_preview { p.sheet_name_action = None; } cx.notify(); })).child(crate::settings::tr("取消", "Cancel"))));
            }
            if let Some(sheet) = book.sheets.get(state.sheet) {
                let hidden_selected = state.selected.is_some_and(|(row, column)| document.session.is_hidden_formula(fdl_core::spreadsheet_edit::Address { sheet: state.sheet, row, column }));
                let row_start = (state.row_page * 30).min(1_048_575) as u32;
                let col_start = (state.col_page * 8).min(16_383) as u32;
                let shown_rows = (row_start..row_start.saturating_add(30).min(1_048_576)).collect::<Vec<_>>();
                let shown_cols = (col_start..col_start.saturating_add(8).min(16_384)).collect::<Vec<_>>();
                let lookup = sheet.cells.iter().filter(|c| shown_rows.contains(&c.row) && shown_cols.contains(&c.column)).map(|c| ((c.row, c.column), c)).collect::<BTreeMap<_, _>>();
                content = content.child(div().px(px(24.)).pb(px(8.)).flex().gap(px(8.)).items_center()
                    .child(div().w(px(110.)).px(px(6.)).py(px(4.)).border_1().border_color(Theme::line_border()).rounded(px(4.)).child(state.address_field.clone()))
                    .child(div().flex_1().min_w_0().px(px(8.)).py(px(4.)).border_1().border_color(Theme::line_border()).rounded(px(4.))
                        .children(state.selected.filter(|_| !hidden_selected).map(|_| state.field.clone()))
                        .when(hidden_selected, |d| d.text_color(Theme::text_caption()).child(crate::settings::tr("工作表已保护，公式已隐藏", "Formula hidden by worksheet protection")))
                        .when(state.selected.is_none(), |d| d.text_color(Theme::text_placeholder()).child(crate::settings::tr("选择单元格后输入", "Select a cell to edit")))));
                content = content.child(div().px(px(24.)).pb(px(8.)).flex().gap(px(10.)).items_center()
                    .child(div().flex_1().min_w_0().px(px(8.)).py(px(4.)).border_1().border_color(Theme::line_border()).rounded(px(4.)).child(state.search_field.clone()))
                    .child(div().id("sheet-find-prev").cursor_pointer().on_click(cx.listener(|w, _, window, cx| w.find_spreadsheet_cell(true, window, cx))).child(crate::settings::tr("上一个", "Previous")))
                    .child(div().id("sheet-find-next").cursor_pointer().on_click(cx.listener(|w, _, window, cx| w.find_spreadsheet_cell(false, window, cx))).child(crate::settings::tr("下一个", "Next"))));
                content = content.child(div().px(px(24.)).pb(px(8.)).flex().flex_wrap().gap(px(10.)).items_center()
                    .child(div().flex_1().min_w(px(180.)).px(px(8.)).py(px(4.)).border_1().border_color(Theme::line_border()).rounded(px(4.)).child(state.replace_field.clone()))
                    .child(div().id("sheet-replace-case").cursor_pointer().text_size(px(11.)).text_color(if state.replace_options.case_sensitive { Theme::primary() } else { Theme::text_caption() }).on_click(cx.listener(|w, _, _, cx| w.toggle_spreadsheet_replace_case(cx))).child(if state.replace_options.case_sensitive { crate::settings::tr("✓ 区分大小写", "✓ Match case") } else { crate::settings::tr("区分大小写", "Match case") }))
                    .child(div().id("sheet-replace-whole").cursor_pointer().text_size(px(11.)).text_color(if state.replace_options.whole_cell { Theme::primary() } else { Theme::text_caption() }).on_click(cx.listener(|w, _, _, cx| w.toggle_spreadsheet_replace_whole(cx))).child(if state.replace_options.whole_cell { crate::settings::tr("✓ 整个单元格", "✓ Whole cell") } else { crate::settings::tr("整个单元格", "Whole cell") }))
                    .child(div().id("sheet-replace-text").cursor_pointer().on_click(cx.listener(|w, _, window, cx| w.replace_spreadsheet_all(fdl_core::spreadsheet_edit::ReplaceMode::Text, window, cx))).child(crate::settings::tr("替换全部文本", "Replace text")))
                    .child(div().id("sheet-replace-formulas").cursor_pointer().on_click(cx.listener(|w, _, window, cx| w.replace_spreadsheet_all(fdl_core::spreadsheet_edit::ReplaceMode::Formula, window, cx))).child(crate::settings::tr("替换全部公式", "Replace formulas")))
                    .child(div().id("sheet-replace-values").cursor_pointer().on_click(cx.listener(|w, _, window, cx| w.replace_spreadsheet_all(fdl_core::spreadsheet_edit::ReplaceMode::Values, window, cx))).child(crate::settings::tr("替换数值原值", "Replace raw values"))
                        .tip_label(crate::settings::tr("按数值原值替换（50% 为 0.5）；日期使用 YYYY-MM-DD", "Replace raw values (50% is 0.5); dates use YYYY-MM-DD").to_string(), TipSide::Above)));
                let mut grid = div().flex().flex_col().child(div().flex().child(grid_cell("#".into(), true)).children(shown_cols.iter().map(|c| grid_cell(column_name(*c), true))));
                for row in shown_rows {
                    let mut line = div().flex().child(grid_cell((row + 1).to_string(), true));
                    for col in &shown_cols {
                        let text = lookup.get(&(row, *col)).map(|cell| {
                            let address = fdl_core::spreadsheet_edit::Address { sheet: state.sheet, row, column: *col };
                            cell_display_text(cell, address, &state.formula_results, &state.formatted_values)
                        }).unwrap_or_default();
                        let col = *col;
                        let index = (row as u64) * 16_384 + u64::from(col);
                        let address = fdl_core::spreadsheet_edit::Address { sheet: state.sheet, row, column: col };
                        let text = document.session.edits().get(&address).filter(|_| !document.session.is_hidden_formula(address)).map(input_text).unwrap_or(text);
                        let selected = state.selected.is_some_and(|(r, c)| {
                            let (end_r, end_c) = state.selection_end.unwrap_or((r, c));
                            (r.min(end_r)..=r.max(end_r)).contains(&row) && (c.min(end_c)..=c.max(end_c)).contains(&col)
                        });
                        let appearance = state.appearances.get(&address);
                        let numeric = matches!(document.session.input(address), Some(fdl_core::spreadsheet_edit::CellInput::Number(_))) || matches!(state.formula_results.get(&address), Some(fdl_core::spreadsheet_calc::FormulaResult::Number(_)));
                        line = line.child(styled_grid_cell(text, appearance, numeric, selected).id(("spreadsheet-cell", index)).cursor_pointer()
                            .on_mouse_down(MouseButton::Left, cx.listener(move |w, ev: &MouseDownEvent, window, cx| {
                                if ev.modifiers.shift { w.select_spreadsheet_range(row, col, window, cx); }
                                else { w.select_spreadsheet_cell(row, col, window, cx); }
                            })));
                    }
                    grid = grid.child(line);
                }
                content = content.child(div().id("spreadsheet-grid-x").flex_1().min_h_0().px(px(24.)).overflow_x_scroll()
                    .child(div().id("spreadsheet-grid-y").w(px(900.)).h_full().overflow_y_scroll().child(grid)));
                let mut arithmetic = div().px(px(24.)).pt(px(8.)).flex().flex_wrap().gap_x(px(10.)).gap_y(px(6.)).items_center()
                    .child(div().text_color(Theme::text_caption()).child(crate::settings::tr("选区运算", "Selection")))
                    .child(div().w(px(90.)).px(px(6.)).py(px(3.)).border_1().border_color(Theme::line_border()).rounded(px(4.)).child(state.arithmetic_field.clone()));
                for (id, symbol, description, operation) in [
                    ("sheet-add", "+", crate::settings::tr("选区加上该数值", "Add to selection"), fdl_core::spreadsheet_edit::Arithmetic::Add),
                    ("sheet-subtract", "−", crate::settings::tr("选区减去该数值", "Subtract from selection"), fdl_core::spreadsheet_edit::Arithmetic::Subtract),
                    ("sheet-multiply", "×", crate::settings::tr("选区乘以该数值", "Multiply selection"), fdl_core::spreadsheet_edit::Arithmetic::Multiply),
                    ("sheet-divide", "÷", crate::settings::tr("选区除以该数值", "Divide selection"), fdl_core::spreadsheet_edit::Arithmetic::Divide),
                ] {
                    arithmetic = arithmetic.child(div().id(id).text_color(if state.selected.is_some() { Theme::text() } else { Theme::text_disabled() })
                        .when(state.selected.is_some(), |d| d.cursor_pointer().on_click(cx.listener(move |w, _, window, cx| w.arithmetic_spreadsheet_range(operation, window, cx))))
                        .child(symbol).tip_label(description.to_string(), TipSide::Above));
                }
                arithmetic = arithmetic.child(div().id("sheet-arithmetic-formulas").cursor_pointer().text_size(px(11.)).text_color(if state.arithmetic_formulas { Theme::primary() } else { Theme::text_caption() })
                    .on_click(cx.listener(|w, _, _, cx| w.toggle_spreadsheet_arithmetic_formulas(cx)))
                    .child(if state.arithmetic_formulas { crate::settings::tr("✓ 包含公式", "✓ Include formulas") } else { crate::settings::tr("包含公式", "Include formulas") })
                    .tip_label(crate::settings::tr("文本、布尔值与日期不参与运算；百分比按原值计算。若公式与其引用的单元格同时在选区，两者都会改变。", "Text, booleans and dates are skipped; percentages use raw values. Selected formulas and their selected input cells are both changed.").to_string(), TipSide::Above));
                content = content.child(arithmetic);
                let mut formats = div().px(px(24.)).pt(px(8.)).flex().flex_wrap().gap_x(px(10.)).gap_y(px(6.)).items_center()
                    .child(div().text_color(Theme::text_caption()).child(crate::settings::tr("数字格式", "Number format")));
                use fdl_core::spreadsheet_xlsx::NumberFormat;
                for (id, label, format) in [
                    ("sheet-format-general", crate::settings::tr("常规", "General"), NumberFormat::General),
                    ("sheet-format-integer", "0", NumberFormat::Integer),
                    ("sheet-format-decimal", "0.00", NumberFormat::Decimal2),
                    ("sheet-format-grouped-integer", "1,000", NumberFormat::GroupedInteger),
                    ("sheet-format-grouped-decimal", "1,000.00", NumberFormat::GroupedDecimal2),
                    ("sheet-format-percent", "0%", NumberFormat::Percent0),
                    ("sheet-format-percent-decimal", "0.00%", NumberFormat::Percent2),
                    ("sheet-format-scientific", "1.00E+00", NumberFormat::Scientific),
                    ("sheet-format-date", crate::settings::tr("日期", "Date"), NumberFormat::Date),
                    ("sheet-format-datetime", crate::settings::tr("日期时间", "Date/time"), NumberFormat::DateTime),
                ] {
                    formats = formats.child(div().id(id).text_size(px(11.)).text_color(if state.selected.is_some() { Theme::text() } else { Theme::text_disabled() })
                        .when(state.selected.is_some(), |d| d.cursor_pointer().on_click(cx.listener(move |w, _, window, cx| w.format_spreadsheet_range(format, window, cx))))
                        .child(label));
                }
                content = content.child(formats);
                let mut controls = div().px(px(24.)).py(px(12.)).flex().flex_wrap().gap_x(px(14.)).gap_y(px(6.));
                controls = controls.child(div().id("sheet-copy-cells").text_color(if state.selected.is_some() { Theme::text() } else { Theme::text_disabled() })
                    .when(state.selected.is_some(), |d| d.cursor_pointer().on_click(cx.listener(|w, _, _, cx| w.copy_spreadsheet_cells(cx))))
                    .child(crate::settings::tr("复制单元格", "Copy cells")));
                controls = controls.child(div().id("sheet-copy-values").text_color(if state.selected.is_some() { Theme::text() } else { Theme::text_disabled() })
                    .when(state.selected.is_some(), |d| d.cursor_pointer().on_click(cx.listener(|w, _, _, cx| w.copy_spreadsheet_values(cx))))
                    .child(crate::settings::tr("复制值", "Copy values")));
                controls = controls.child(div().id("sheet-clear-range").text_color(if state.selected.is_some() { Theme::text() } else { Theme::text_disabled() })
                    .when(state.selected.is_some(), |d| d.cursor_pointer().on_click(cx.listener(|w, _, window, cx| w.fill_spreadsheet_range(true, window, cx))))
                    .child(crate::settings::tr("清空内容", "Clear contents")));
                controls = controls.child(div().id("sheet-fill-range").text_color(if state.selection_end.is_some() { Theme::text() } else { Theme::text_disabled() })
                    .when(state.selection_end.is_some(), |d| d.cursor_pointer().on_click(cx.listener(|w, _, window, cx| w.fill_spreadsheet_range(false, window, cx))))
                    .child(crate::settings::tr("填充选区", "Fill range")));
                for (id, label, enabled, undo) in [
                    ("sheet-undo", crate::settings::tr("撤销", "Undo"), document.session.can_undo(), true),
                    ("sheet-redo", crate::settings::tr("重做", "Redo"), document.session.can_redo(), false),
                ] {
                    controls = controls.child(div().id(id).text_color(if enabled { Theme::text() } else { Theme::text_disabled() }).when(enabled, |d| d.cursor_pointer().on_click(cx.listener(move |w, _, window, cx| w.spreadsheet_history(undo, window, cx)))).child(label));
                }
                for (id, label, enabled, dr, dc) in [
                    ("row-prev", crate::settings::tr("上一组行", "Previous rows"), state.row_page > 0, -1, 0),
                    ("row-next", crate::settings::tr("下一组行", "Next rows"), row_start + 30 < 1_048_576, 1, 0),
                    ("col-prev", crate::settings::tr("上一组列", "Previous columns"), state.col_page > 0, 0, -1),
                    ("col-next", crate::settings::tr("下一组列", "Next columns"), col_start + 8 < 16_384, 0, 1),
                ] {
                    controls = controls.child(div().id(id).text_color(if enabled { Theme::text() } else { Theme::text_disabled() }).when(enabled, |d| d.cursor_pointer().on_click(cx.listener(move |w, _, _, cx| {
                        if let Some(p) = &mut w.spreadsheet_preview { p.row_page = p.row_page.saturating_add_signed(dr); p.col_page = p.col_page.saturating_add_signed(dc); }
                        cx.notify();
                    }))).child(label));
                }
                content = content.child(controls);
                if document.session.is_dirty() {
                    content = content.child(div().px(px(24.)).pb(px(8.)).flex().gap(px(18.))
                        .child(div().id("spreadsheet-retry").cursor_pointer().text_color(Theme::primary())
                            .on_click(cx.listener(|w, _, window, cx| w.apply_spreadsheet_edit(window, cx))).child(crate::settings::tr("重试保存", "Retry save")))
                        .child(div().id("spreadsheet-save-copy").cursor_pointer().text_color(Theme::primary())
                            .on_click(cx.listener(|w, _, window, cx| w.save_spreadsheet_copy(window, cx))).child(crate::settings::tr("另存副本", "Save a copy"))));
                }
            }
        } else {
            content = content.child(div().px(px(24.)).child(if state.saving { crate::settings::tr("正在保存…", "Saving…") } else { crate::settings::tr("正在读取…", "Loading…") }));
        }
        if state.confirm_discard {
            content = content.child(div().px(px(24.)).pb(px(12.)).id("spreadsheet-discard").cursor_pointer().text_color(Theme::primary())
                .on_click(cx.listener(|w, _, _, cx| { w.spreadsheet_preview = None; cx.notify(); }))
                .child(crate::settings::tr("确认放弃未保存的修改", "Confirm discard unsaved changes")));
        }
        Some(crate::transfer::mask("spreadsheet-mask", content, |w, cx| w.close_spreadsheet_preview(cx), cx))
    }
}

fn grid_cell(text: String, header: bool) -> Div {
    div().w(px(100.)).flex_shrink_0().h(px(30.)).px(px(6.)).border_1().border_color(Theme::line_divider())
        .when(header, |d| d.bg(Theme::bg_overlay())).overflow_hidden().child(text)
}

fn styled_grid_cell(text: String, style: Option<&fdl_core::spreadsheet_calc::CellAppearance>, numeric: bool, selected: bool) -> Div {
    use fdl_core::spreadsheet_calc::CellAlignment;
    let mut cell = grid_cell(text, false);
    if let Some(style) = style {
        // Workbook fills remain exact; unbacked text adapts to the app theme.
        if let Some(background) = style.background { cell = cell.bg(rgb(background)); }
        if let Some(foreground) = style.foreground {
            cell = if style.background.is_some() { cell.text_color(rgb(foreground)) } else { cell.text_color(crate::theme::c(foreground)) };
        } else if let Some(background) = style.background {
            let linear = |component: u32| { let value = component as f32 / 255.; if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) } };
            let luminance = 0.2126 * linear((background >> 16) & 255) + 0.7152 * linear((background >> 8) & 255) + 0.0722 * linear(background & 255);
            cell = cell.text_color(rgb(if luminance > 0.179 { 0x000000 } else { 0xffffff }));
        }
        if style.bold { cell = cell.font_weight(FontWeight::BOLD); }
        if style.italic { cell = cell.italic(); }
        if style.underline { cell = cell.underline(); }
        cell = match style.alignment {
            CellAlignment::Left => cell,
            CellAlignment::Center => cell.text_center(),
            CellAlignment::Right => cell.text_right(),
            CellAlignment::General if numeric => cell.text_right(),
            CellAlignment::General => cell,
        };
        if !selected {
            cell = cell.relative();
            for (index, edge) in style.borders.iter().enumerate() {
                if let Some(edge) = edge {
                    let color = edge.color.map_or_else(Theme::text, |color| if style.background.is_some() { rgb(color).into() } else { crate::theme::c(color) });
                    let line = div().absolute().bg(color);
                    let line = match index {
                        0 => line.left_0().top_0().bottom_0().w(px(edge.width as f32)),
                        1 => line.left_0().right_0().top_0().h(px(edge.width as f32)),
                        2 => line.right_0().top_0().bottom_0().w(px(edge.width as f32)),
                        _ => line.left_0().right_0().bottom_0().h(px(edge.width as f32)),
                    };
                    cell = cell.child(line);
                }
            }
        }
    } else if numeric { cell = cell.text_right(); }
    if selected {
        if style.is_none_or(|style| style.background.is_none()) { cell = cell.bg(Theme::fill_turninto_active()); }
        cell = cell.border_color(Theme::primary());
    }
    cell
}

#[cfg(feature = "devtools")]
pub(crate) fn check_preview(phase: &str, cx: &mut App) {
    let root = PathBuf::from(std::env::var_os("MARKDOWNX_TEST_WORKSPACE_ROOT").expect("test root"));
    assert!(root.starts_with(std::env::temp_dir()) && root.join(".markdownx-workspace-test").is_file());
    let path = root.join("fixture.xlsx");
    let handle = cx.windows().into_iter().find_map(|w| w.downcast::<Workspace>()).unwrap();
    handle.update(cx, |w, window, cx| {
        #[cfg(target_os = "windows")]
        assert!(window.bounds().origin.x < px(-10000.), "test window must remain off-screen");
        match phase {
            "start" => w.open_external_path(path.clone(), window, cx),
            "done" => {
                let preview = w.spreadsheet_preview.as_ref().expect("preview");
                let book = &preview.document.as_ref().expect("loaded workbook").session.workbook;
                assert_eq!(book.sheets.len(), 2);
                assert_eq!(book.sheets[0].name, "数据");
                assert!(book.sheets[0].cells.iter().any(|cell| cell.formula.as_deref() == Some("SUM(1,2)")));
                w.select_spreadsheet_cell(2, 3, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("123", cx));
                w.schedule_spreadsheet_edit(window, cx);
            }
            "saved" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.document.as_ref().unwrap().session.is_dirty());
                assert!(fdl_core::spreadsheet::read(&path).unwrap().sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == 123.0));
                w.spreadsheet_history(true, window, cx);
            }
            "undone" => {
                assert!(fdl_core::spreadsheet::read(&path).unwrap().sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == "中文"));
                w.spreadsheet_history(false, window, cx);
            }
            "redone" => {
                assert!(fdl_core::spreadsheet::read(&path).unwrap().sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == 123.0));
            }
            "jump" => {
                w.spreadsheet_preview.as_ref().unwrap().address_field.update(cx, |field, cx| {
                    field.set_text("$xfd$1048576", cx);
                    cx.emit(TextFieldEvent::Submit);
                });
            }
            "jump-done" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert_eq!(preview.selected, Some((1_048_575, 16_383)));
                assert_eq!(preview.address_field.read(cx).text, "XFD1048576");
                preview.address_field.update(cx, |field, cx| field.set_text("XFE1", cx));
                w.jump_spreadsheet_address(window, cx);
                assert_eq!(w.spreadsheet_preview.as_ref().unwrap().selected, Some((1_048_575, 16_383)));
                w.select_spreadsheet_cell(2, 3, window, cx);
            }
            "nav-start" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert_eq!(preview.selected, Some((2, 3)));
                preview.field.update(cx, |field, cx| field.set_text("321", cx));
                w.navigate_spreadsheet(0, 1, window, cx);
                assert!(w.spreadsheet_preview.as_ref().unwrap().saving);
            }
            "nav-done" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving);
                assert_eq!(preview.selected, Some((2, 4)));
                assert!(fdl_core::spreadsheet::read(&path).unwrap().sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == 321.0));
            }
            "nav-key" => assert_eq!(w.spreadsheet_preview.as_ref().unwrap().selected, Some((2, 5))),
            "key-undone" => assert!(fdl_core::spreadsheet::read(&path).unwrap().sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == 123.0)),
            "key-redone" => assert!(fdl_core::spreadsheet::read(&path).unwrap().sheets[0].cells.iter().any(|c| c.row == 2 && c.column == 3 && c.value == 321.0)),
            "paste" => {
                w.select_spreadsheet_cell(0, 0, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| {
                    field.pending_table_paste = Some("生命\t20\n法力\t50".into());
                    cx.emit(TextFieldEvent::PasteTable);
                });
            }
            "pasted" => {
                let book = fdl_core::spreadsheet::read(&path).unwrap();
                assert!(book.sheets[0].cells.iter().any(|c| c.row == 0 && c.column == 0 && c.value == "生命"));
                assert!(book.sheets[0].cells.iter().any(|c| c.row == 1 && c.column == 1 && c.value == 50.0));
                w.select_spreadsheet_range(1, 1, window, cx);
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert_eq!(preview.selected, Some((0, 0)));
                assert_eq!(preview.selection_end, Some((1, 1)));
                let a = fdl_core::spreadsheet_edit::Address { sheet: 0, row: 0, column: 0 };
                let b = fdl_core::spreadsheet_edit::Address { row: 1, column: 1, ..a };
                assert_eq!(preview.document.as_ref().unwrap().session.copy_values_tsv(a, b, &preview.formula_results).unwrap(), "生命\t20\n法力\t50");
                w.spreadsheet_history(true, window, cx);
            }
            "paste-undone" => {
                let book = fdl_core::spreadsheet::read(&path).unwrap();
                assert!(!book.sheets[0].cells.iter().any(|c| c.row == 0 && c.column == 0));
                assert!(!book.sheets[0].cells.iter().any(|c| c.row == 1 && c.column == 1));
                w.spreadsheet_history(false, window, cx);
            }
            "paste-redone" => {
                let book = fdl_core::spreadsheet::read(&path).unwrap();
                assert!(book.sheets[0].cells.iter().any(|c| c.row == 0 && c.column == 0 && c.value == "生命"));
                assert!(book.sheets[0].cells.iter().any(|c| c.row == 1 && c.column == 1 && c.value == 50.0));
            }
            "find-start" => {
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(p.search_field.read(cx).is_focused(window));
                p.search_field.update(cx, |field, cx| { field.set_text("法力", cx); cx.emit(TextFieldEvent::Submit); });
            }
            "find-done" => {
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert_eq!(p.selected, Some((1, 0)));
                assert!(p.search_field.read(cx).is_focused(window));
                p.search_field.update(cx, |field, cx| field.set_text("生命", cx));
                w.find_spreadsheet_cell(true, window, cx);
                assert_eq!(w.spreadsheet_preview.as_ref().unwrap().selected, Some((0, 0)));
                w.select_spreadsheet_range(1, 1, window, cx);
            }
            "clear-start" => w.fill_spreadsheet_range(true, window, cx),
            "clear-saved" => {
                let book = fdl_core::spreadsheet::read(&path).unwrap();
                assert!(!book.sheets[0].cells.iter().any(|c| c.row <= 1 && c.column <= 1));
                w.spreadsheet_history(true, window, cx);
            }
            "clear-undone" => {
                let book = fdl_core::spreadsheet::read(&path).unwrap();
                assert!(book.sheets[0].cells.iter().any(|c| c.row == 0 && c.column == 0 && c.value == "生命"));
                assert!(book.sheets[0].cells.iter().any(|c| c.row == 1 && c.column == 1 && c.value == 50.0));
                w.fill_spreadsheet_range(false, window, cx);
            }
            "fill-saved" => {
                let book = fdl_core::spreadsheet::read(&path).unwrap();
                assert_eq!(book.sheets[0].cells.iter().filter(|c| c.row <= 1 && c.column <= 1 && c.value == "生命").count(), 4);
                w.spreadsheet_history(true, window, cx);
            }
            "fill-undone" => {
                let book = fdl_core::spreadsheet::read(&path).unwrap();
                assert!(book.sheets[0].cells.iter().any(|c| c.row == 0 && c.column == 0 && c.value == "生命"));
                assert!(book.sheets[0].cells.iter().any(|c| c.row == 1 && c.column == 1 && c.value == 50.0));
            }
            "conflict" => {
                std::fs::write(&path, b"external change").unwrap();
                w.select_spreadsheet_cell(4, 5, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("777", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "verify" => {
                assert!(w.spreadsheet_has_pending(cx));
                assert_eq!(std::fs::read(&path).unwrap(), b"external change");
                w.close_spreadsheet_preview(cx);
                assert!(w.spreadsheet_preview.as_ref().unwrap().confirm_discard);
                assert!(!w.save_before(cx, |_, _, _| {}));
            }
            "copy" => w.save_spreadsheet_copy(window, cx),
            "copy-done" => {
                let recovered = root.join("recovered.xlsx");
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.confirm_discard);
                let document = preview.document.as_ref().unwrap();
                assert_eq!(document.path, recovered);
                assert!(!document.session.is_dirty());
                assert_eq!(std::fs::read(&path).unwrap(), b"external change");
                let book = fdl_core::spreadsheet::read(&recovered).unwrap();
                assert!(book.sheets[0].cells.iter().any(|c| c.row == 4 && c.column == 5 && c.value == 777.0));
                w.close_spreadsheet_preview(cx);
                assert!(w.spreadsheet_preview.is_none());
            }
            "replace-start" => w.open_external_path(root.join("replacements.xlsx"), window, cx),
            "replace-loaded" => w.select_spreadsheet_cell(0, 0, window, cx),
            "replace-text" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(preview.search_field.read(cx).is_focused(window));
                preview.search_field.update(cx, |field, cx| field.set_text("HP", cx));
                preview.replace_field.update(cx, |field, cx| field.set_text("MP", cx));
                w.replace_spreadsheet_all(fdl_core::spreadsheet_edit::ReplaceMode::Text, window, cx);
            }
            "replace-text-saved" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                let session = &p.document.as_ref().unwrap().session;
                assert_eq!(session.input(Address { sheet: 0, row: 0, column: 0 }), Some(CellInput::Text("生命 MP MP".into())));
                assert_eq!(session.input(Address { sheet: 0, row: 1, column: 0 }), Some(CellInput::Text("MP 冷却".into())));
                assert_eq!(session.input(Address { sheet: 0, row: 0, column: 1 }), Some(CellInput::Formula("A1&\"HP\"".into())));
                assert_eq!(p.field.read(cx).text, "'生命 MP MP");
                w.spreadsheet_history(true, window, cx);
            }
            "replace-text-undone" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                assert_eq!(p.document.as_ref().unwrap().session.input(Address { sheet: 0, row: 0, column: 0 }), Some(CellInput::Text("生命 HP HP".into())));
                w.spreadsheet_history(false, window, cx);
            }
            "replace-text-redone" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput, ReplaceMode};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                assert_eq!(p.document.as_ref().unwrap().session.input(Address { sheet: 0, row: 0, column: 0 }), Some(CellInput::Text("生命 MP MP".into())));
                p.search_field.update(cx, |field, cx| field.set_text("A1", cx));
                p.replace_field.update(cx, |field, cx| field.set_text("A2", cx));
                w.replace_spreadsheet_all(ReplaceMode::Formula, window, cx);
            }
            "replace-formula-saved" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                let session = &p.document.as_ref().unwrap().session;
                assert_eq!(session.input(Address { sheet: 0, row: 0, column: 1 }), Some(CellInput::Formula("A2&\"HP\"".into())));
                assert_eq!(session.input(Address { sheet: 1, row: 0, column: 0 }), Some(CellInput::Text("HP".into())));
                w.spreadsheet_history(true, window, cx);
            }
            "replace-formula-undone" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                assert_eq!(p.document.as_ref().unwrap().session.input(Address { sheet: 0, row: 0, column: 1 }), Some(CellInput::Formula("A1&\"HP\"".into())));
                w.spreadsheet_history(false, window, cx);
            }
            "replace-formula-redone" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                assert_eq!(p.document.as_ref().unwrap().session.input(Address { sheet: 0, row: 0, column: 1 }), Some(CellInput::Formula("A2&\"HP\"".into())));
                w.toggle_spreadsheet_replace_case(cx);
                w.toggle_spreadsheet_replace_whole(cx);
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.replace_options.case_sensitive && p.replace_options.whole_cell);
                p.search_field.update(cx, |field, cx| field.set_text("true", cx));
                p.replace_field.update(cx, |field, cx| field.set_text("FALSE", cx));
                w.replace_spreadsheet_all(fdl_core::spreadsheet_edit::ReplaceMode::Values, window, cx);
            }
            "replace-boolean-saved" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput, ReplaceMode};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                assert_eq!(p.document.as_ref().unwrap().session.input(Address { sheet: 0, row: 0, column: 5 }), Some(CellInput::Boolean(false)));
                p.search_field.update(cx, |field, cx| field.set_text("0.125", cx));
                p.replace_field.update(cx, |field, cx| field.set_text("0.5", cx));
                w.replace_spreadsheet_all(ReplaceMode::Values, window, cx);
            }
            "replace-percentage-saved" => {
                use fdl_core::spreadsheet_edit::{Address, ReplaceMode};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating && !p.calculation_failed);
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 0, column: 3 }], "50.00%");
                p.search_field.update(cx, |field, cx| field.set_text("2025-01-02", cx));
                p.replace_field.update(cx, |field, cx| field.set_text("2026-02-03", cx));
                w.replace_spreadsheet_all(ReplaceMode::Values, window, cx);
            }
            "replace-date-saved" => {
                use fdl_core::spreadsheet_edit::Address;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating && !p.calculation_failed);
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 0, column: 4 }], "2026-02-03");
                w.spreadsheet_history(true, window, cx);
            }
            "replace-date-undone" => {
                use fdl_core::spreadsheet_edit::Address;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating && !p.calculation_failed);
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 0, column: 4 }], "2025-01-02");
                w.spreadsheet_history(false, window, cx);
            }
            "replace-date-redone" => {
                use fdl_core::spreadsheet_edit::Address;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating && !p.calculation_failed);
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 0, column: 4 }], "2026-02-03");
                w.close_spreadsheet_preview(cx);
            }
            "arithmetic-start" => w.open_external_path(root.join("arithmetic.xlsx"), window, cx),
            "arithmetic-multiply" => {
                w.select_spreadsheet_cell(0, 0, window, cx);
                w.select_spreadsheet_range(3, 1, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().arithmetic_field.update(cx, |field, cx| field.set_text("2", cx));
                w.arithmetic_spreadsheet_range(fdl_core::spreadsheet_edit::Arithmetic::Multiply, window, cx);
            }
            "arithmetic-multiplied" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating && !p.calculation_failed);
                let session = &p.document.as_ref().unwrap().session;
                assert_eq!(session.input(Address { sheet: 0, row: 0, column: 0 }), Some(CellInput::Number(20.0)));
                assert_eq!(session.input(Address { sheet: 0, row: 1, column: 0 }), Some(CellInput::Number(40.0)));
                assert_eq!(session.input(Address { sheet: 0, row: 1, column: 1 }), Some(CellInput::Number(0.5)));
                assert_eq!(session.input(Address { sheet: 0, row: 0, column: 1 }), Some(CellInput::Formula("A1*2".into())));
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 3, column: 0 }], "2025-01-02");
                w.spreadsheet_history(true, window, cx);
            }
            "arithmetic-undone" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                assert_eq!(p.document.as_ref().unwrap().session.input(Address { sheet: 0, row: 0, column: 0 }), Some(CellInput::Number(10.0)));
                w.spreadsheet_history(false, window, cx);
            }
            "arithmetic-redone" => {
                use fdl_core::spreadsheet_edit::Arithmetic;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                w.toggle_spreadsheet_arithmetic_formulas(cx);
                w.spreadsheet_preview.as_ref().unwrap().arithmetic_field.update(cx, |field, cx| field.set_text("10%", cx));
                w.arithmetic_spreadsheet_range(Arithmetic::Add, window, cx);
            }
            "arithmetic-added" => {
                use fdl_core::spreadsheet_edit::{Address, Arithmetic, CellInput};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating && !p.calculation_failed);
                let session = &p.document.as_ref().unwrap().session;
                assert_eq!(session.input(Address { sheet: 0, row: 0, column: 0 }), Some(CellInput::Number(20.1)));
                assert_eq!(session.input(Address { sheet: 0, row: 0, column: 1 }), Some(CellInput::Formula("(A1*2)+(0.1)".into())));
                assert_eq!(session.input(Address { sheet: 0, row: 2, column: 0 }), Some(CellInput::Text("notes".into())));
                assert_eq!(session.input(Address { sheet: 0, row: 2, column: 1 }), Some(CellInput::Boolean(true)));
                let before = std::fs::read(root.join("arithmetic.xlsx")).unwrap();
                p.arithmetic_field.update(cx, |field, cx| field.set_text("0", cx));
                w.arithmetic_spreadsheet_range(Arithmetic::Divide, window, cx);
                assert_eq!(std::fs::read(root.join("arithmetic.xlsx")).unwrap(), before);
                assert!(!w.spreadsheet_has_pending(cx));
                w.spreadsheet_history(true, window, cx);
            }
            "arithmetic-add-undone" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                assert_eq!(p.document.as_ref().unwrap().session.input(Address { sheet: 0, row: 0, column: 0 }), Some(CellInput::Number(20.0)));
                w.spreadsheet_history(false, window, cx);
            }
            "arithmetic-add-redone" => {
                use fdl_core::spreadsheet_edit::{Address, CellInput};
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                assert_eq!(p.document.as_ref().unwrap().session.input(Address { sheet: 0, row: 0, column: 0 }), Some(CellInput::Number(20.1)));
                w.close_spreadsheet_preview(cx);
            }
            "calc-start" => w.open_external_path(root.join("calculation.xlsx"), window, cx),
            "calc-loaded" => {
                use fdl_core::spreadsheet_calc::FormulaResult;
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formula_results.get(&Address { sheet: 0, row: 0, column: 1 }), Some(&FormulaResult::Number(30.0)));
                assert_eq!(preview.formula_results.get(&Address { sheet: 1, row: 0, column: 0 }), Some(&FormulaResult::Number(60.0)));
                w.select_spreadsheet_cell(0, 0, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("15", cx));
                w.schedule_spreadsheet_edit(window, cx);
                assert!(w.spreadsheet_preview.as_ref().unwrap().formula_results.is_empty());
            }
            "calc-after" => {
                use fdl_core::spreadsheet_calc::FormulaResult;
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.calculating && !preview.calculation_failed && !preview.saving);
                assert_eq!(preview.formula_results.get(&Address { sheet: 0, row: 0, column: 1 }), Some(&FormulaResult::Number(35.0)));
                assert_eq!(preview.formula_results.get(&Address { sheet: 1, row: 0, column: 0 }), Some(&FormulaResult::Number(70.0)));
                w.select_spreadsheet_cell(0, 1, window, cx);
                w.select_spreadsheet_range(0, 2, window, cx);
                w.fill_spreadsheet_range(false, window, cx);
            }
            "formula-fill-saved" => {
                let book = fdl_core::spreadsheet::read(&root.join("calculation.xlsx")).unwrap();
                assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 2 && cell.formula.as_deref() == Some("SUM(B1:B2)")));
                w.spreadsheet_history(true, window, cx);
            }
            "formula-fill-undone" => {
                let book = fdl_core::spreadsheet::read(&root.join("calculation.xlsx")).unwrap();
                assert!(!book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 2 && cell.formula.is_some()));
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                let source = fdl_core::spreadsheet_edit::Address { sheet: 0, row: 0, column: 1 };
                let cells = preview.document.as_ref().unwrap().session.copy_cells(source, source).unwrap();
                let payload = SpreadsheetClipboard { source_id: preview.id.to_string(), cells };
                let metadata = format!("{}{}", crate::input::SPREADSHEET_CLIPBOARD_PREFIX, serde_json::to_string(&payload).unwrap());
                let foreign = format!("{}{}", crate::input::SPREADSHEET_CLIPBOARD_PREFIX, serde_json::to_string(&SpreadsheetClipboard { source_id: "other".into(), cells: payload.cells.clone() }).unwrap());
                w.select_spreadsheet_cell(1, 3, window, cx);
                w.paste_spreadsheet_cells(&foreign, window, cx);
                assert!(!w.spreadsheet_has_pending(cx));
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| {
                    field.pending_table_paste = Some("=SUM(A1:A2)".into());
                    field.pending_table_metadata = Some(metadata);
                    cx.emit(TextFieldEvent::PasteTable);
                });
            }
            "clipboard-pasted" => {
                let book = fdl_core::spreadsheet::read(&root.join("calculation.xlsx")).unwrap();
                assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 1 && cell.column == 3 && cell.formula.as_deref() == Some("SUM(C2:C3)")));
                w.spreadsheet_history(true, window, cx);
            }
            "clipboard-undone" => {
                let book = fdl_core::spreadsheet::read(&root.join("calculation.xlsx")).unwrap();
                assert!(!book.sheets[0].cells.iter().any(|cell| cell.row == 1 && cell.column == 3 && cell.formula.is_some()));
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                let source = fdl_core::spreadsheet_edit::Address { sheet: 0, row: 0, column: 1 };
                let cells = preview.document.as_ref().unwrap().session.copy_cells(source, source).unwrap();
                let metadata = format!("{}{}", crate::input::SPREADSHEET_CLIPBOARD_PREFIX, serde_json::to_string(&SpreadsheetClipboard { source_id: preview.id.to_string(), cells }).unwrap());
                w.select_spreadsheet_sheet(1, cx);
                w.select_spreadsheet_cell(1, 3, window, cx);
                w.paste_spreadsheet_cells(&metadata, window, cx);
            }
            "clipboard-cross-saved" => {
                let book = fdl_core::spreadsheet::read(&root.join("calculation.xlsx")).unwrap();
                assert!(book.sheets[1].cells.iter().any(|cell| cell.row == 1 && cell.column == 3 && cell.formula.as_deref() == Some("SUM(C2:C3)")));
                w.spreadsheet_history(true, window, cx);
            }
            "clipboard-cross-undone" => {
                let book = fdl_core::spreadsheet::read(&root.join("calculation.xlsx")).unwrap();
                assert!(!book.sheets[1].cells.iter().any(|cell| cell.row == 1 && cell.column == 3 && cell.formula.is_some()));
                w.select_spreadsheet_sheet(0, cx);
                w.prompt_spreadsheet_sheet_name(SheetNameAction::Rename(0), window, cx);
                w.spreadsheet_preview.as_ref().unwrap().sheet_name_field.update(cx, |field, cx| {
                    field.set_text("原数据", cx);
                    cx.emit(TextFieldEvent::Submit);
                });
            }
            "sheet-reference-refused" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && preview.sheet_name_action.is_some());
                let book = fdl_core::spreadsheet::read(&preview.path).unwrap();
                assert_eq!(book.sheets[0].name, "Values");
                assert!(book.sheets[1].cells.iter().any(|cell| cell.formula.as_deref() == Some("Values!B1*2")));
                w.prompt_delete_spreadsheet_sheet(cx);
                assert_eq!(w.spreadsheet_preview.as_ref().unwrap().confirm_sheet_delete, Some(0));
                w.delete_spreadsheet_sheet(window, cx);
            }
            "sheet-delete-refused" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && preview.confirm_sheet_delete == Some(0));
                let book = fdl_core::spreadsheet::read(&preview.path).unwrap();
                assert_eq!(book.sheets.len(), 2);
                assert_eq!(book.sheets[0].name, "Values");
                w.close_spreadsheet_preview(cx);
                assert!(w.spreadsheet_preview.is_none());
            }
            "protected-start" => w.open_external_path(root.join("protected_ranges.xlsx"), window, cx),
            "protected-edit" => {
                w.select_spreadsheet_cell(1, 1, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("123", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "protected-check" => {
                assert!(w.spreadsheet_has_pending(cx));
                assert_eq!(std::fs::read(root.join("protected_ranges.xlsx")).unwrap(), include_bytes!("../crates/core/tests/fixtures/protected_ranges.xlsx"));
                w.spreadsheet_history(true, window, cx);
                assert!(!w.spreadsheet_has_pending(cx));
                let p = w.spreadsheet_preview.as_ref().unwrap();
                p.search_field.update(cx, |field, cx| field.set_text("A1", cx));
                p.replace_field.update(cx, |field, cx| field.set_text("B1", cx));
                w.replace_spreadsheet_all(fdl_core::spreadsheet_edit::ReplaceMode::Formula, window, cx);
            }
            "protected-replace-refused" => {
                assert_eq!(std::fs::read(root.join("protected_ranges.xlsx")).unwrap(), include_bytes!("../crates/core/tests/fixtures/protected_ranges.xlsx"));
                assert!(!w.spreadsheet_has_pending(cx));
                w.close_spreadsheet_preview(cx);
                assert!(w.spreadsheet_preview.is_none());
            }
            "sheet-protection-start" => w.open_external_path(root.join("sheet_protection.xlsx"), window, cx),
            "sheet-protection-locked" => {
                w.select_spreadsheet_cell(0, 0, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("pending", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "sheet-protection-blocked" => {
                assert!(w.spreadsheet_has_pending(cx));
                assert_eq!(std::fs::read(root.join("sheet_protection.xlsx")).unwrap(), include_bytes!("../crates/core/tests/fixtures/sheet_protection.xlsx"));
                w.spreadsheet_history(true, window, cx);
                assert!(!w.spreadsheet_has_pending(cx));
                w.select_spreadsheet_cell(0, 1, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("changed", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "sheet-protection-unlocked" => {
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                let book = fdl_core::spreadsheet::read(&p.path).unwrap();
                assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 1 && cell.value == "changed"));
                p.search_field.update(cx, |field, cx| field.set_text("e", cx));
                p.replace_field.update(cx, |field, cx| field.set_text("X", cx));
                w.replace_spreadsheet_all(fdl_core::spreadsheet_edit::ReplaceMode::Text, window, cx);
            }
            "sheet-protection-batch-blocked" => {
                assert!(!w.spreadsheet_has_pending(cx));
                let p = w.spreadsheet_preview.as_ref().unwrap();
                let book = fdl_core::spreadsheet::read(&p.path).unwrap();
                assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 0 && cell.value == "locked"));
                assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 1 && cell.value == "changed"));
                w.select_spreadsheet_cell(0, 1, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| {
                    field.pending_table_paste = Some("updated\t11\t22".into());
                    cx.emit(TextFieldEvent::PasteTable);
                });
            }
            "sheet-protection-pasted" => {
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving);
                let book = fdl_core::spreadsheet::read(&p.path).unwrap();
                assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 2 && cell.value == 11.0));
                assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 3 && cell.value == 22.0));
                w.select_spreadsheet_cell(1, 0, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("row", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "sheet-protection-row-saved" => {
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !w.spreadsheet_has_pending(cx));
                let book = fdl_core::spreadsheet::read(&p.path).unwrap();
                assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 1 && cell.column == 0 && cell.value == "row"));
                w.close_spreadsheet_preview(cx);
            }
            "conditional-start" => w.open_external_path(root.join("conditional_appearance.xlsx"), window, cx),
            "conditional-loaded" => {
                use fdl_core::spreadsheet_edit::Address;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.calculating && !p.conditional_unavailable);
                assert_eq!(p.appearances.get(&Address { sheet: 0, row: 0, column: 0 }).unwrap().background, None);
                let styled = p.appearances.get(&Address { sheet: 0, row: 1, column: 0 }).unwrap();
                assert_eq!(styled.background, Some(0xffc7ce));
                assert_eq!(styled.borders[2].unwrap().color, Some(0x0000ff));
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 0, column: 3 }], "12.50%");
                w.select_spreadsheet_cell(0, 0, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("250", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "conditional-edited" => {
                use fdl_core::spreadsheet_edit::Address;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating);
                assert_eq!(p.appearances.get(&Address { sheet: 0, row: 0, column: 0 }).unwrap().background, Some(0xffc7ce));
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 2, column: 0 }], "1000");
                w.spreadsheet_history(true, window, cx);
            }
            "conditional-undone" => {
                use fdl_core::spreadsheet_edit::Address;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating);
                assert_eq!(p.appearances.get(&Address { sheet: 0, row: 0, column: 0 }).unwrap().background, None);
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 2, column: 0 }], "200");
            }
            "conditional-large-start" => w.open_external_path(root.join("conditional_oversized.xlsx"), window, cx),
            "conditional-large-loaded" => {
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(p.conditional_unavailable && !p.calculation_failed && !p.calculating);
                assert_eq!(p.formatted_values[&fdl_core::spreadsheet_edit::Address { sheet: 0, row: 1, column: 0 }], "200");
            }
            "appearance-start" => w.open_external_path(root.join("cell_appearance.xlsx"), window, cx),
            "appearance-loaded" => {
                use fdl_core::spreadsheet_calc::CellAlignment;
                use fdl_core::spreadsheet_edit::Address;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                let style = p.appearances.get(&Address { sheet: 0, row: 0, column: 0 }).unwrap();
                assert_eq!(style.background, Some(0xffc7ce));
                assert_eq!(style.foreground, Some(0x9c0006));
                assert!(style.bold && style.italic && style.underline);
                assert_eq!(style.alignment, CellAlignment::Center);
                assert_eq!(p.appearances.get(&Address { sheet: 0, row: 1, column: 2 }).unwrap().background, Some(0xffeb9c));
                w.select_spreadsheet_cell(0, 0, window, cx);
                assert_eq!(w.spreadsheet_preview.as_ref().unwrap().field.read(cx).text, "'ATK");
            }
            "hidden-formula-start" => w.open_external_path(root.join("hidden_formulas.xlsx"), window, cx),
            "hidden-formula-selected" => {
                w.select_spreadsheet_cell(0, 1, window, cx);
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(p.document.as_ref().unwrap().session.is_hidden_formula(fdl_core::spreadsheet_edit::Address { sheet: 0, row: 0, column: 1 }));
                assert!(p.field.read(cx).text.is_empty());
                assert_eq!(p.formatted_values[&fdl_core::spreadsheet_edit::Address { sheet: 0, row: 0, column: 1 }], "20");
                w.select_spreadsheet_cell(0, 2, window, cx);
            }
            "hidden-formula-visible" => {
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert_eq!(p.field.read(cx).text, "=A1+3");
            }
            "hidden-new-start" => w.open_external_path(root.join("hidden_new.xlsx"), window, cx),
            "hidden-new-write" => {
                w.select_spreadsheet_cell(0, 4, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("=A1*3", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "hidden-new-saved" => {
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && p.document.as_ref().unwrap().session.is_hidden_formula(fdl_core::spreadsheet_edit::Address { sheet: 0, row: 0, column: 4 }));
                assert!(p.field.read(cx).text.is_empty());
                w.spreadsheet_history(true, window, cx);
            }
            "hidden-new-undone" => {
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.document.as_ref().unwrap().session.is_hidden_formula(fdl_core::spreadsheet_edit::Address { sheet: 0, row: 0, column: 4 }));
            }
            "number-style-start" => w.open_external_path(root.join("number_formats.xlsx"), window, cx),
            "number-style-apply" => {
                w.select_spreadsheet_cell(0, 0, window, cx);
                w.format_spreadsheet_range(fdl_core::spreadsheet_xlsx::NumberFormat::Decimal2, window, cx);
            }
            "number-style-saved" => {
                use fdl_core::spreadsheet_edit::Address;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating);
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 0, column: 0 }], "0.13");
                w.spreadsheet_history(true, window, cx);
            }
            "number-style-undone" => {
                use fdl_core::spreadsheet_edit::Address;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating);
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 0, column: 0 }], "12.50%");
                w.spreadsheet_history(false, window, cx);
            }
            "number-style-redone" => {
                use fdl_core::spreadsheet_edit::Address;
                let p = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!p.saving && !p.calculating);
                assert_eq!(p.formatted_values[&Address { sheet: 0, row: 0, column: 0 }], "0.13");
            }
            "format-start" => w.open_external_path(root.join("number_formats.xlsx"), window, cx),
            "format-loaded" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.calculating && !preview.calculation_failed);
                let document = preview.document.as_ref().unwrap();
                let first = document.session.workbook.sheets[0].cells.iter().find(|cell| cell.row == 0 && cell.column == 0).unwrap();
                let address = Address { sheet: 0, row: 0, column: 0 };
                assert_eq!(cell_display_text(first, address, &preview.formula_results, &preview.formatted_values), "12.50%");
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 3, column: 0 }], "2025-01-02");
                assert_eq!(document.session.find_cell(0, "12.50%", None, false, &preview.formula_results, &preview.formatted_values), Some(address));
                w.select_spreadsheet_cell(1, 0, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("50%", cx));
                w.apply_spreadsheet_edit(window, cx);
                assert!(w.spreadsheet_has_pending(cx));
                assert!(!w.spreadsheet_preview.as_ref().unwrap().document.as_ref().unwrap().session.is_dirty());
                w.spreadsheet_history(true, window, cx);
                w.select_spreadsheet_cell(0, 0, window, cx);
                assert!(w.spreadsheet_preview.as_ref().unwrap().calculating);
                w.spreadsheet_preview.as_mut().unwrap().percentage_styles.clear();
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("50%", cx));
                w.schedule_spreadsheet_edit(window, cx);
                assert!(w.spreadsheet_preview.as_ref().unwrap().formatted_values.is_empty());
            }
            "format-saved" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 0, column: 0 }], "50.00%");
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 0, column: 1 }], "100.0%");
                let book = fdl_core::spreadsheet::read(&preview.path).unwrap();
                assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 0 && cell.value == 0.5));
                w.select_spreadsheet_cell(2, 0, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| {
                    field.pending_table_paste = Some("25%".into());
                    cx.emit(TextFieldEvent::PasteTable);
                });
            }
            "format-pasted" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 2, column: 0 }], "25%");
                let book = fdl_core::spreadsheet::read(&preview.path).unwrap();
                assert!(book.sheets[0].cells.iter().any(|cell| cell.row == 2 && cell.column == 0 && cell.value == 0.25));
                w.select_spreadsheet_cell(3, 1, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("50%", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "format-explicit-saved" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 3, column: 1 }], "50.00%");
                w.select_spreadsheet_cell(4, 2, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| {
                    field.pending_table_paste = Some("25%".into());
                    cx.emit(TextFieldEvent::PasteTable);
                });
            }
            "format-column-saved" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 4, column: 2 }], "25%");
                w.select_spreadsheet_cell(5, 0, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| {
                    field.pending_table_paste = Some("75%\t40%\t25%\t2".into());
                    cx.emit(TextFieldEvent::PasteTable);
                });
            }
            "format-row-saved" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                for (column, expected) in [(0, "75.0%"), (1, "40.0%"), (2, "25.0%"), (3, "2")] {
                    assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 5, column }], expected);
                }
                w.select_spreadsheet_cell(3, 0, window, cx);
                assert_eq!(w.spreadsheet_preview.as_ref().unwrap().field.read(cx).text, "2025-01-02");
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("2026-02-03", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "format-date-saved" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 3, column: 0 }], "2026-02-03");
                w.spreadsheet_history(true, window, cx);
            }
            "format-date-undone" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 3, column: 0 }], "2025-01-02");
                w.spreadsheet_history(false, window, cx);
            }
            "format-date-redone" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 3, column: 0 }], "2026-02-03");
                w.select_spreadsheet_cell(6, 1, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("2027-01-02", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "format-blank-date-saved" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 6, column: 1 }], "2027-01-02");
                w.select_spreadsheet_cell(6, 5, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| {
                    field.pending_table_paste = Some("2026-02-04\n2026-02-05".into());
                    cx.emit(TextFieldEvent::PasteTable);
                });
            }
            "format-inherited-date-saved" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 6, column: 5 }], "2026-02-04");
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 7, column: 5 }], "2026-02-05");
                w.select_spreadsheet_cell(6, 1, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "format-date-cleared" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                w.select_spreadsheet_cell(0, 0, window, cx);
                w.select_spreadsheet_cell(6, 1, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("2028-03-04", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "format-date-restored" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && !preview.calculation_failed);
                assert_eq!(preview.formatted_values[&Address { sheet: 0, row: 6, column: 1 }], "2028-03-04");
                w.close_spreadsheet_preview(cx);
            }
            "date-1904-start" => w.open_external_path(root.join("dates_1904.xlsx"), window, cx),
            "date-1904-loaded" => {
                use fdl_core::spreadsheet_edit::Address;
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.calculating && preview.calculation_failed);
                let document = preview.document.as_ref().unwrap();
                let cell = document.session.workbook.sheets[0].cells.iter().find(|cell| cell.row == 0 && cell.column == 0).unwrap();
                assert_eq!(cell_display_text(cell, Address { sheet: 0, row: 0, column: 0 }, &preview.formula_results, &preview.formatted_values), "2025-01-02");
                w.select_spreadsheet_cell(0, 0, window, cx);
                assert_eq!(w.spreadsheet_preview.as_ref().unwrap().field.read(cx).text, "2025-01-02");
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("2025-01-03", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "date-1904-saved" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && preview.calculation_failed);
                let book = fdl_core::spreadsheet::read(&preview.path).unwrap();
                assert_eq!(fdl_core::spreadsheet_edit::fallback_cell_text(&book.sheets[0].cells[0]), "2025-01-03");
                w.select_spreadsheet_cell(1, 4, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("2025-01-04", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "date-1904-blank-saved" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(!preview.saving && !preview.calculating && preview.calculation_failed);
                let book = fdl_core::spreadsheet::read(&preview.path).unwrap();
                let cell = book.sheets[0].cells.iter().find(|cell| (cell.row, cell.column) == (1, 4)).unwrap();
                assert_eq!(fdl_core::spreadsheet_edit::fallback_cell_text(cell), "2025-01-04");
                w.close_spreadsheet_preview(cx);
            }
            "new" => {
                let workspace = w.workspace_roots.iter().find(|workspace| workspace.tree.is_some() && workspace.error.is_none()).unwrap();
                assert!(workspace.spec.path.starts_with(&root));
                let id = workspace.id;
                w.prompt_workspace_spreadsheet(id, None, window, cx);
                w.workspace_rename.as_ref().unwrap().field.update(cx, |field, cx| {
                    field.set_text("游戏数值", cx);
                    cx.emit(TextFieldEvent::Submit);
                });
            }
            "new-done" => {
                let preview = w.spreadsheet_preview.as_ref().expect("new workbook opens");
                assert!(preview.path.starts_with(&root));
                assert_eq!(preview.path.file_name().unwrap(), "游戏数值.xlsx");
                let workbook = &preview.document.as_ref().expect("new workbook loaded").session.workbook;
                assert_eq!(workbook.sheets.len(), 1);
                assert!(workbook.sheets[0].cells.is_empty());
                w.prompt_spreadsheet_sheet_name(SheetNameAction::Add, window, cx);
            }
            "sheet-prompt" => {
                assert!(w.spreadsheet_preview.as_ref().unwrap().sheet_name_field.read(cx).is_focused(window));
                w.spreadsheet_preview.as_ref().unwrap().sheet_name_field.update(cx, |field, cx| {
                    field.set_text("游戏数值", cx);
                    cx.emit(TextFieldEvent::Submit);
                });
            }
            "sheet-added" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                let workbook = &preview.document.as_ref().unwrap().session.workbook;
                assert_eq!(workbook.sheets.len(), 2);
                assert_eq!(workbook.sheets[1].name, "游戏数值");
                assert_eq!(preview.sheet, 1);
                assert_eq!(preview.selected, Some((0, 0)));
                preview.field.update(cx, |field, cx| field.set_text("99", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "sheet-edited" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                let book = fdl_core::spreadsheet::read(&preview.path).unwrap();
                assert!(book.sheets[1].cells.iter().any(|cell| cell.row == 0 && cell.column == 0 && cell.value == 99.0));
                w.prompt_spreadsheet_sheet_name(SheetNameAction::Rename(1), window, cx);
                w.spreadsheet_preview.as_ref().unwrap().sheet_name_field.update(cx, |field, cx| {
                    field.set_text("数值平衡", cx);
                    cx.emit(TextFieldEvent::Submit);
                });
            }
            "sheet-renamed" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                let book = fdl_core::spreadsheet::read(&preview.path).unwrap();
                assert_eq!(book.sheets[1].name, "数值平衡");
                assert!(book.sheets[1].cells.iter().any(|cell| cell.row == 0 && cell.column == 0 && cell.value == 99.0));
                w.prompt_spreadsheet_sheet_name(SheetNameAction::Add, window, cx);
                w.spreadsheet_preview.as_ref().unwrap().sheet_name_field.update(cx, |field, cx| {
                    field.set_text("临时", cx);
                    cx.emit(TextFieldEvent::Submit);
                });
            }
            "sheet-delete-setup" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert_eq!(preview.sheet, 2);
                preview.field.update(cx, |field, cx| field.set_text("77", cx));
                w.apply_spreadsheet_edit(window, cx);
            }
            "sheet-delete-confirm" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                assert!(preview.document.as_ref().unwrap().session.can_undo());
                w.prompt_delete_spreadsheet_sheet(cx);
                assert_eq!(w.spreadsheet_preview.as_ref().unwrap().confirm_sheet_delete, Some(2));
                assert_eq!(fdl_core::spreadsheet::read(&w.spreadsheet_preview.as_ref().unwrap().path).unwrap().sheets.len(), 3);
                w.delete_spreadsheet_sheet(window, cx);
            }
            "sheet-delete-done" => {
                let preview = w.spreadsheet_preview.as_ref().unwrap();
                let document = preview.document.as_ref().unwrap();
                assert!(!document.session.can_undo() && !document.session.can_redo());
                let book = fdl_core::spreadsheet::read(&preview.path).unwrap();
                assert_eq!(book.sheets.len(), 2);
                assert_eq!(book.sheets[1].name, "数值平衡");
                assert!(book.sheets[1].cells.iter().any(|cell| cell.row == 0 && cell.column == 0 && cell.value == 99.0));
                w.close_spreadsheet_preview(cx);
            }
            _ => panic!("unknown spreadsheet preview test phase"),
        }
    }).unwrap();
    eprintln!("spreadsheet-preview: {phase} PASS");
}

#[cfg(test)]
mod tests {
    use super::parse_address;

    #[test]
    fn parses_bounded_excel_addresses() {
        for (input, expected) in [("A1", Some((0, 0))), (" $xfd$1048576 ", Some((1_048_575, 16_383))), ("aa32", Some((31, 26))), ("XFE1", None), ("A1048577", None), ("A0", None), ("A1:B2", None), ("1A", None), ("Ａ1", None), ("A-1", None), ("", None)] {
            assert_eq!(parse_address(input), expected, "{input}");
        }
    }
}
