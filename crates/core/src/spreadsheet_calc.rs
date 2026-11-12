//! Non-destructive local calculation for XLSX formula cells.
//!
//! This calculates an in-memory model only; it does not rewrite the source
//! archive or claim support for every Excel function.
use std::collections::BTreeMap;
use anyhow::{Result, ensure};
use ironcalc::base::{Model, cell::CellValue, types::CellType};
use crate::spreadsheet_edit::Address;

#[derive(Debug, Clone, PartialEq)]
pub enum FormulaResult {
    Empty,
    Number(f64),
    Text(String),
    Boolean(bool),
    Error(String),
}

#[derive(Debug)]
pub struct Evaluation {
    pub formulas: BTreeMap<Address, FormulaResult>,
    pub formatted: BTreeMap<Address, String>,
    pub percentage_styles: PercentageStyles,
    pub appearances: AppearanceStyles,
    pub calculation_unavailable: bool,
    pub conditional_unavailable: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CellAlignment { #[default] General, Left, Center, Right }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellBorder { pub color: Option<u32>, pub width: u8 }

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellAppearance {
    /// Exact workbook RGB values, not application theme tokens.
    pub background: Option<u32>,
    pub foreground: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub alignment: CellAlignment,
    /// Left, top, right and bottom. Dashed/double styles are approximated.
    pub borders: [Option<CellBorder>; 4],
}

#[derive(Debug, Clone, Default)]
pub struct AppearanceStyles { palette: Vec<CellAppearance>, sheets: Vec<SheetStyleIndices> }

#[derive(Debug, Clone, Default)]
struct SheetStyleIndices {
    rows: BTreeMap<u32, usize>,
    columns: Vec<(u32, u32, usize)>,
    cells: BTreeMap<(u32, u32), usize>,
    conditional: BTreeMap<(u32, u32), CellAppearance>,
    conditional_formats: BTreeMap<(u32, u32), String>,
}

impl AppearanceStyles {
    pub fn get(&self, address: &Address) -> Option<&CellAppearance> {
        let sheet = self.sheets.get(address.sheet)?;
        if let Some(style) = sheet.conditional.get(&(address.row, address.column)) { return Some(style); }
        let column_index = sheet.columns.partition_point(|(_, last, _)| *last < address.column);
        let column = sheet.columns.get(column_index).filter(|(first, last, _)| (*first..=*last).contains(&address.column)).map(|(_, _, style)| *style);
        let style = sheet.cells.get(&(address.row, address.column)).copied().or_else(|| sheet.rows.get(&address.row).copied()).or(column).unwrap_or(0);
        self.palette.get(style)
    }
    pub fn clear(&mut self) { self.sheets.clear(); self.palette.clear(); }
    fn conditional_format(&self, address: &Address) -> Option<&str> { self.sheets.get(address.sheet)?.conditional_formats.get(&(address.row, address.column)).map(String::as_str) }
}

fn color_rgb(color: &ironcalc::base::types::Color, theme: &ironcalc::base::types::Theme) -> Option<u32> {
    let value = color.to_rgb(theme);
    let value = value.strip_prefix('#')?;
    if value.len() != 6 || !value.is_ascii() { return None; }
    u32::from_str_radix(value, 16).ok()
}

fn appearance(font: &ironcalc::base::types::Font, fill: &ironcalc::base::types::Fill, alignment: Option<&ironcalc::base::types::Alignment>, border: &ironcalc::base::types::Border, default_font: bool, theme: &ironcalc::base::types::Theme) -> CellAppearance {
    use ironcalc::base::types::{BorderItem, BorderStyle, HorizontalAlignment};
    let background = color_rgb(&fill.color, theme);
    let foreground = if default_font && background.is_none() { None } else { color_rgb(&font.color, theme) };
    let alignment = match alignment.map(|alignment| &alignment.horizontal) {
        Some(HorizontalAlignment::Left) => CellAlignment::Left,
        Some(HorizontalAlignment::Center | HorizontalAlignment::CenterContinuous) => CellAlignment::Center,
        Some(HorizontalAlignment::Right) => CellAlignment::Right,
        _ => CellAlignment::General,
    };
    let edge = |item: &Option<BorderItem>| item.as_ref().map(|item| CellBorder {
        color: color_rgb(&item.color, theme),
        width: match item.style { BorderStyle::Thick | BorderStyle::Double => 3, BorderStyle::Medium | BorderStyle::MediumDashed | BorderStyle::MediumDashDot | BorderStyle::MediumDashDotDot => 2, _ => 1 },
    });
    CellAppearance { background, foreground, bold: font.b, italic: font.i, underline: font.u, alignment,
        borders: [edge(&border.left), edge(&border.top), edge(&border.right), edge(&border.bottom)] }
}

fn appearance_styles(model: &Model<'_>, conditional: Option<&[std::collections::BTreeSet<(i32, i32)>]>) -> Result<(AppearanceStyles, bool)> {
    let styles = &model.workbook.styles;
    let mut palette = Vec::with_capacity(styles.cell_xfs.len());
    for xf in &styles.cell_xfs {
        let font = styles.fonts.get(usize::try_from(xf.font_id)?).ok_or_else(|| anyhow::anyhow!("invalid cell font index"))?;
        let fill = styles.fills.get(usize::try_from(xf.fill_id)?).ok_or_else(|| anyhow::anyhow!("invalid cell fill index"))?;
        let border = styles.borders.get(usize::try_from(xf.border_id)?).ok_or_else(|| anyhow::anyhow!("invalid cell border index"))?;
        palette.push(appearance(font, fill, xf.alignment.as_ref(), border, xf.font_id == 0, &model.workbook.theme));
    }
    ensure!(!palette.is_empty(), "cell styles are missing");
    let index = |value: i32| -> Result<usize> { let value = usize::try_from(value)?; ensure!(value < palette.len(), "invalid cell style index"); Ok(value) };
    let mut sheets = Vec::with_capacity(model.workbook.worksheets.len());
    let mut unsupported = false;
    for (sheet_index, sheet) in model.workbook.worksheets.iter().enumerate() {
        let mut state = SheetStyleIndices::default();
        for row in &sheet.rows { if row.custom_format { state.rows.insert((row.r - 1) as u32, index(row.s)?); } }
        let columns = sheet.cols.iter().map(|column| Ok(((column.min - 1) as u32, (column.max - 1) as u32, index(column.style.unwrap_or(0))?))).collect::<Result<Vec<_>>>()?;
        state.columns = resolve_column_styles(&columns);
        for (row, column, cell) in sheet.sheet_data.cells() { state.cells.insert(((row - 1) as u32, (column - 1) as u32), index(cell.get_style())?); }
        if let Some(positions) = conditional.and_then(|sheets| sheets.get(sheet_index)) {
            for &(row, column) in positions {
                let extended = model.get_extended_style_for_cell(sheet_index as u32, row, column).map_err(|e| anyhow::anyhow!("conditional style: {e}"))?;
                if extended.icon.is_some() || extended.data_bar.is_some() || extended.rating.is_some() { unsupported = true; }
                let base = model.get_style_for_cell(sheet_index as u32, row, column).map_err(|e| anyhow::anyhow!("cell style: {e}"))?;
                if extended.style != base {
                    let position = ((row - 1) as u32, (column - 1) as u32);
                    let style = &extended.style;
                    state.conditional.insert(position, appearance(&style.font, &style.fill, style.alignment.as_ref(), &style.border, styles.fonts.first() == Some(&style.font), &model.workbook.theme));
                    if style.num_fmt != base.num_fmt { state.conditional_formats.insert(position, style.num_fmt.clone()); }
                }
            }
        }
        sheets.push(state);
    }
    Ok((AppearanceStyles { palette, sheets }, unsupported))
}

/// Bound the calculation engine's conditional-format traversal before model
/// construction: a single full-column rule can otherwise visit a million cells.
fn conditional_positions(workbook: &ironcalc::base::types::Workbook) -> Result<Vec<std::collections::BTreeSet<(i32, i32)>>> {
    use ironcalc::base::expressions::utils::parse_reference_a1;
    let mut total = 0u64;
    let mut lookup_cost = 0u64;
    let mut sheets = Vec::with_capacity(workbook.worksheets.len());
    for sheet in &workbook.worksheets {
        let mut positions = std::collections::BTreeSet::new();
        for rule in &sheet.conditional_formatting {
            for part in rule.range.split_whitespace() {
                let upper = part.to_ascii_uppercase();
                let (first, last) = upper.split_once(':').unwrap_or((&upper, &upper));
                let a = parse_reference_a1(first).ok_or_else(|| anyhow::anyhow!("unsupported conditional format range"))?;
                let b = parse_reference_a1(last).ok_or_else(|| anyhow::anyhow!("unsupported conditional format range"))?;
                ensure!((1..=1_048_576).contains(&a.row) && (1..=1_048_576).contains(&b.row) && (1..=16_384).contains(&a.column) && (1..=16_384).contains(&b.column), "invalid conditional format range");
                let (top, bottom) = (a.row.min(b.row), a.row.max(b.row));
                let (left, right) = (a.column.min(b.column), a.column.max(b.column));
                let area = (bottom - top + 1) as u64 * (right - left + 1) as u64;
                total = total.checked_add(area).ok_or_else(|| anyhow::anyhow!("conditional format range too large"))?;
                ensure!(total <= 100_000, "conditional format range too large");
                let scan = sheet.rows.len().saturating_add(sheet.cols.len()).saturating_add(1) as u64;
                lookup_cost = lookup_cost.saturating_add(area.saturating_mul(scan));
                ensure!(lookup_cost <= 5_000_000, "conditional format lookup too large");
                for row in top..=bottom { for column in left..=right { positions.insert((row, column)); } }
            }
        }
        sheets.push(positions);
    }
    Ok(sheets)
}

#[derive(Debug, Clone, Default)]
pub struct PercentageStyles { sheets: Vec<SheetPercentageStyles>, is_1904: bool }

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FormatFlags { percentage: bool, calendar_date: bool }

#[derive(Debug, Clone, Default)]
struct SheetPercentageStyles {
    default: FormatFlags,
    rows: BTreeMap<u32, FormatFlags>,
    columns: Vec<(u32, u32, FormatFlags)>,
    cells: BTreeMap<(u32, u32), FormatFlags>,
}

impl PercentageStyles {
    fn flags(&self, address: &Address) -> Option<FormatFlags> {
        let sheet = self.sheets.get(address.sheet)?;
        if let Some(value) = sheet.cells.get(&(address.row, address.column)) { return Some(*value); }
        if let Some(value) = sheet.rows.get(&address.row) { return Some(*value); }
        let index = sheet.columns.partition_point(|(_, last, _)| *last < address.column);
        if let Some((first, last, value)) = sheet.columns.get(index) && (*first..=*last).contains(&address.column) { return Some(*value); }
        Some(sheet.default)
    }

    pub fn get(&self, address: &Address) -> Option<bool> { self.flags(address).map(|flags| flags.percentage) }

    /// The workbook epoch is returned only when the effective format contains
    /// calendar fields. This includes blank cells inheriting row/column styles.
    pub fn date_epoch(&self, address: &Address) -> Option<bool> {
        self.flags(address).filter(|flags| flags.calendar_date).map(|_| self.is_1904)
    }

    pub fn clear(&mut self) { self.sheets.clear(); }

    #[cfg(test)]
    pub(crate) fn test_cells(cells: &BTreeMap<Address, bool>) -> Self {
        let count = cells.keys().map(|a| a.sheet + 1).max().unwrap_or(0);
        let mut sheets = vec![SheetPercentageStyles::default(); count];
        for (address, value) in cells { sheets[address.sheet].cells.insert((address.row, address.column), FormatFlags { percentage: *value, calendar_date: false }); }
        Self { sheets, is_1904: false }
    }
}

fn percentage_styles(model: &Model<'_>, is_1904: bool) -> Result<PercentageStyles> {
    let styles = &model.workbook.styles;
    let style_flags = styles.cell_xfs.iter().map(|xf| {
        let format = ironcalc::base::number_format::get_num_fmt(xf.num_fmt_id, &styles.num_fmts);
        FormatFlags { percentage: is_percentage_format(&format), calendar_date: is_calendar_date_format(&format) }
    }).collect::<Vec<_>>();
    let flag = |index: i32| -> Result<FormatFlags> { Ok(*style_flags.get(usize::try_from(index)?).ok_or_else(|| anyhow::anyhow!("invalid cell style index"))?) };
    let default = flag(0)?;
    let mut sheets = Vec::with_capacity(model.workbook.worksheets.len());
    let mut total_cells = 0usize;
    for sheet in &model.workbook.worksheets {
        let mut state = SheetPercentageStyles { default, ..Default::default() };
        let mut seen_rows = std::collections::BTreeSet::new();
        for row in &sheet.rows {
            ensure!((1..=1_048_576).contains(&row.r) && seen_rows.insert(row.r), "invalid or duplicate row style");
            if row.custom_format { state.rows.insert((row.r - 1) as u32, flag(row.s)?); }
        }
        let mut columns = Vec::with_capacity(sheet.cols.len());
        for column in &sheet.cols {
            ensure!((1..=16_384).contains(&column.min) && (column.min..=16_384).contains(&column.max), "invalid column style range");
            columns.push(((column.min - 1) as u32, (column.max - 1) as u32, flag(column.style.unwrap_or(0))?));
        }
        state.columns = resolve_column_styles(&columns);
        for (row, column, cell) in sheet.sheet_data.cells() {
            total_cells += 1;
            ensure!(total_cells <= 100_000 && (1..=1_048_576).contains(&row) && (1..=16_384).contains(&column), "workbook exceeds local style limit");
            ensure!(state.cells.insert(((row - 1) as u32, (column - 1) as u32), flag(cell.get_style())?).is_none(), "duplicate styled cell");
        }
        sheets.push(state);
    }
    Ok(PercentageStyles { sheets, is_1904 })
}

/// Normalize overlapping column declarations into sorted, disjoint intervals.
/// The first declaration wins, matching the calculation engine's lookup. A
/// next-unassigned set keeps even heavily overlapping XLSX files linear.
fn resolve_column_styles<T: Copy + PartialEq>(columns: &[(u32, u32, T)]) -> Vec<(u32, u32, T)> {
    if columns.is_empty() { return Vec::new(); }
    const LIMIT: usize = 16_384;
    fn next_unassigned(parent: &mut [usize], mut index: usize) -> usize {
        let mut root = index;
        while parent[root] != root { root = parent[root]; }
        while parent[index] != index { let next = parent[index]; parent[index] = root; index = next; }
        root
    }
    let mut parent = (0..=LIMIT).collect::<Vec<_>>();
    let mut values = vec![None; LIMIT];
    for &(first, last, flags) in columns {
        let mut index = next_unassigned(&mut parent, first as usize);
        while index <= last as usize {
            values[index] = Some(flags);
            let next = next_unassigned(&mut parent, index + 1);
            parent[index] = next;
            index = next;
        }
    }
    let mut intervals: Vec<(u32, u32, T)> = Vec::new();
    for (column, flags) in values.into_iter().enumerate() {
        let Some(flags) = flags else { continue };
        let column = column as u32;
        if let Some((_, end, previous)) = intervals.last_mut() && *end + 1 == column && *previous == flags { *end = column; }
        else { intervals.push((column, column, flags)); }
    }
    intervals
}

/// Recognize unescaped year/day tokens in the first number-format section.
/// Time-only and elapsed-time formats deliberately remain non-calendar.
fn is_calendar_date_format(format: &str) -> bool {
    let mut quoted = false;
    let mut bracketed = false;
    let mut skip_next = false;
    for c in format.chars() {
        if skip_next { skip_next = false; continue; }
        if !quoted && !bracketed && matches!(c, '\\' | '_' | '*') { skip_next = true; continue; }
        if !bracketed && c == '"' { quoted = !quoted; continue; }
        if !quoted && c == '[' { bracketed = true; continue; }
        if !quoted && c == ']' { bracketed = false; continue; }
        if !quoted && !bracketed && c == ';' { break; }
        if !quoted && !bracketed && matches!(c, 'y' | 'Y' | 'd' | 'D') { return true; }
    }
    false
}

/// Whether a number format contains an actual percentage operator, excluding
/// quoted or escaped literal percent signs and bracketed format metadata.
pub fn is_percentage_format(format: &str) -> bool {
    let mut quoted = false;
    let mut bracketed = false;
    let mut skip_next = false;
    let mut percentage = false;
    for c in format.chars() {
        if skip_next { skip_next = false; continue; }
        if !quoted && !bracketed && matches!(c, '\\' | '_' | '*') { skip_next = true; continue; }
        if !bracketed && c == '"' { quoted = !quoted; continue; }
        if !quoted && c == '[' { bracketed = true; continue; }
        if !quoted && c == ']' { bracketed = false; continue; }
        if !quoted && !bracketed && c == '%' { percentage = true; }
    }
    percentage && !quoted && !bracketed && !skip_next
}

#[derive(Clone, Copy)]
struct RoundingSpec { precision: i32, percent: i32, comma: i32, scientific: bool, scientific_minus: bool, simple: bool }

/// IronCalc 0.9.1 formats exact decimal midpoints using ties-to-even. Excel's
/// displayed number formats round those midpoints away from zero. Restrict the
/// correction to parsed, unconditional number sections; never change the value
/// held in the model or the source workbook.
fn rounding_specs(format: &str) -> Vec<Option<RoundingSpec>> {
    use ironcalc::base::formatter::parser::{ParsePart, Parser, TextToken};
    let mut parser = Parser::new(format);
    parser.parse();
    if parser.parts.len() > 4 || parser.parts.iter().any(|part| matches!(part, ParsePart::Number(part) if part.condition.is_some())) { return Vec::new(); }
    parser.parts.iter().map(|part| match part {
        ParsePart::Number(part) if (0..=15).contains(&part.precision) && (0..=8).contains(&part.percent) && (0..=8).contains(&part.comma)
            && (!part.is_scientific || part.digit_count == 1) && !part.tokens.iter().any(|token| matches!(token, TextToken::Raw)) =>
            Some(RoundingSpec { precision: part.precision, percent: part.percent, comma: part.comma, scientific: part.is_scientific, scientific_minus: part.scientific_minus,
                simple: part.currency.is_none() && part.tokens.iter().all(|token| matches!(token, TextToken::Digit(_) | TextToken::Period | TextToken::Literal('%'))) }),
        _ => None,
    }).collect()
}

/// Return an adjusted value only for an exact decimal midpoint. Digits are
/// taken from the shortest round-tripping decimal representation, avoiding a
/// multiply-then-round error such as 1.005 * 100 becoming 100.49999....
fn midpoint_away_from_zero(value: f64, spec: RoundingSpec) -> Option<f64> {
    if !value.is_finite() || value == 0.0 { return None; }
    let text = value.abs().to_string();
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (text.as_str(), 0),
    };
    let integer_len = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits = mantissa.bytes().filter(|byte| *byte != b'.').collect::<Vec<_>>();
    let decimal_pos = integer_len.checked_add(exponent)?;
    let cut = if spec.scientific {
        i32::try_from(digits.iter().position(|digit| *digit != b'0')?).ok()?.checked_add(1 + spec.precision)?
    } else {
        decimal_pos.checked_add(spec.precision)?.checked_add(2 * spec.percent)?.checked_sub(3 * spec.comma)?
    };
    let cut = usize::try_from(cut).ok()?;
    if cut >= digits.len() || digits[cut] != b'5' || digits[cut + 1..].iter().any(|digit| *digit != b'0') { return None; }
    let mut prefix = digits[..cut].to_vec();
    let mut carry = true;
    for digit in prefix.iter_mut().rev() {
        if *digit == b'9' { *digit = b'0'; } else { *digit += 1; carry = false; break; }
    }
    if carry { prefix.insert(0, b'1'); }
    let exponent = decimal_pos.checked_sub(i32::try_from(cut).ok()?)?;
    let adjusted = format!("{}e{exponent}", std::str::from_utf8(&prefix).ok()?).parse::<f64>().ok()?;
    if !adjusted.is_finite() || adjusted == 0.0 { return None; }
    Some(adjusted.copysign(value))
}

fn corrected_number_text(value: f64, format: &str, original: &str, cache: &mut BTreeMap<String, Vec<Option<RoundingSpec>>>) -> Option<String> {
    let sections = cache.entry(format.to_string()).or_insert_with(|| rounding_specs(format));
    let index = match sections.len() {
        1 => 0,
        2 => usize::from(value < 0.0),
        3 | 4 => if value > 0.0 { 0 } else if value < 0.0 { 1 } else { 2 },
        _ => return None,
    };
    let spec = sections.get(index).copied().flatten()?;
    let rounded = midpoint_away_from_zero(value, spec);
    let display_value = rounded.unwrap_or(value);
    let mut text = if rounded.is_some() {
        let result = ironcalc::base::number_format::format_number(display_value, format, "en");
        if result.error.is_some() { return None; }
        result.text
    } else { original.to_string() };
    // IronCalc emits E-00 for 1 <= |value| < 10. An exponent of zero is
    // nonnegative; respect the format's E+ versus E- sign convention.
    let scaled = display_value.abs() * 100.0_f64.powi(spec.percent) / 1000.0_f64.powi(spec.comma);
    if spec.scientific && (1.0..10.0).contains(&scaled) && text.matches("E-").count() == 1 {
        let (head, tail) = text.split_once("E-")?;
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && tail.as_bytes()[..digits].iter().all(|digit| *digit == b'0') {
            text = format!("{head}E{}{tail}", if spec.scientific_minus { "" } else { "+" });
        }
    }
    // The engine drops the sign for small negative values in these built-in
    // formats, including values that round to negative zero.
    if value < 0.0 && sections.len() == 1 && spec.simple && !text.starts_with('-') {
        text.insert(0, '-');
    }
    if text == original { None } else { Some(text) }
}

/// Calculate formula values from the exact workbook snapshot. The calculation
/// engine is isolated from the lossless archive writer and never saves files.
pub fn calculate(snapshot: &[u8]) -> Result<BTreeMap<Address, FormulaResult>> {
    let evaluation = evaluate(snapshot)?;
    ensure!(!evaluation.calculation_unavailable, "1904 date system is not supported by the calculation engine");
    Ok(evaluation.formulas)
}

/// Evaluate formulas and compute formatted display text without writing the
/// workbook. Unsupported workbooks remain available through the raw reader.
pub fn evaluate(snapshot: &[u8]) -> Result<Evaluation> {
    std::panic::catch_unwind(|| evaluate_inner(snapshot))
        .map_err(|_| anyhow::anyhow!("formula engine failed while evaluating this workbook"))?
}

fn evaluate_inner(snapshot: &[u8]) -> Result<Evaluation> {
    let (source, is_1904) = crate::spreadsheet::read_xlsx_snapshot_with_epoch(snapshot)?;
    // IronCalc 0.9.1 assumes the 1900 epoch, including in DATE and other
    // functions. Its formula values must not be shown for 1904 files.
    let cell_count = source.sheets.iter().map(|s| s.cells.len()).sum::<usize>();
    ensure!(cell_count <= 100_000, "workbook exceeds local calculation cell limit");
    // Some engines traverse worksheet dimensions even when cells are sparse.
    // Keep extreme coordinate spans out of the in-process evaluator.
    for sheet in &source.sheets {
        if let Some(first) = sheet.cells.first() {
            let mut min = (first.row, first.column);
            let mut max = min;
            for cell in &sheet.cells { min.0 = min.0.min(cell.row); min.1 = min.1.min(cell.column); max.0 = max.0.max(cell.row); max.1 = max.1.max(cell.column); }
            let area = u64::from(max.0 - min.0 + 1) * u64::from(max.1 - min.1 + 1);
            ensure!(area <= 2_000_000, "worksheet span exceeds local calculation limit");
        }
    }
    let mut input = ironcalc::import::load_from_xlsx_bytes(snapshot, "workbook", "en", "UTC")
        .map_err(|e| anyhow::anyhow!("formula engine cannot import workbook: {e}"))?;
    let has_conditional = input.worksheets.iter().any(|sheet| !sheet.conditional_formatting.is_empty());
    let positions = if is_1904 { None } else { conditional_positions(&input).ok() };
    if positions.is_none() { for sheet in &mut input.worksheets { sheet.conditional_formatting.clear(); } }
    let mut model = Model::from_workbook(input, "en").map_err(|e| anyhow::anyhow!("formula model: {e}"))?;
    let names = model.get_worksheets_properties();
    ensure!(names.len() == source.sheets.len() && names.iter().zip(&source.sheets).all(|(a, b)| a.name == b.name), "calculation sheet mapping differs from workbook");
    let percentage_styles = percentage_styles(&model, is_1904)?;
    if !is_1904 { model.evaluate(); }
    let (appearances, unsupported_conditional) = appearance_styles(&model, positions.as_deref())?;
    let mut results = BTreeMap::new();
    let mut formatted = BTreeMap::new();
    let mut rounding_cache = BTreeMap::new();
    let mut formatted_bytes = 0usize;
    for (sheet, data) in source.sheets.iter().enumerate() {
        for cell in &data.cells {
            let sheet_index = u32::try_from(sheet)?;
            let row = i32::try_from(cell.row + 1)?;
            let column = i32::try_from(cell.column + 1)?;
            let address = Address { sheet, row: cell.row, column: cell.column };
            if is_1904 && cell.formula.is_some() { continue; }
            if cell.formula.is_some() || matches!(cell.value, calamine::Data::Float(_) | calamine::Data::Int(_) | calamine::Data::DateTime(_)) {
                let mut text = if is_1904 && matches!(cell.value, calamine::Data::DateTime(date) if date.is_datetime()) {
                    crate::spreadsheet_edit::fallback_cell_text(cell)
                } else {
                    model.get_formatted_cell_value(sheet_index, row, column).map_err(|e| anyhow::anyhow!("formatted cell value: {e}"))?
                };
                if !is_1904 || !matches!(cell.value, calamine::Data::DateTime(date) if date.is_datetime()) {
                    let value = model.get_cell_value_by_index(sheet_index, row, column).map_err(|e| anyhow::anyhow!("cell value: {e}"))?;
                    if let CellValue::Number(value) = value {
                        let style = model.get_style_for_cell(sheet_index, row, column).map_err(|e| anyhow::anyhow!("cell style: {e}"))?;
                        let format = appearances.conditional_format(&address).unwrap_or(&style.num_fmt);
                        if format != style.num_fmt {
                            let result = ironcalc::base::number_format::format_number(value, format, "en");
                            if result.error.is_none() { text = result.text; }
                        }
                        if let Some(corrected) = corrected_number_text(value, format, &text, &mut rounding_cache) { text = corrected; }
                    }
                }
                formatted_bytes = formatted_bytes.checked_add(text.len()).ok_or_else(|| anyhow::anyhow!("formatted values exceed limit"))?;
                ensure!(formatted_bytes <= 32 * 1024 * 1024, "formatted values exceed limit");
                formatted.insert(address, text);
            }
            if cell.formula.is_none() { continue; }
            let kind = model.get_cell_type(sheet_index, row, column).map_err(|e| anyhow::anyhow!("formula cell type: {e}"))?;
            let value = model.get_cell_value_by_index(sheet_index, row, column).map_err(|e| anyhow::anyhow!("formula value: {e}"))?;
            let value = if kind == CellType::ErrorValue {
                FormulaResult::Error(match value { CellValue::String(s) => s, _ => "#ERROR!".into() })
            } else {
                match value {
                    CellValue::None => FormulaResult::Empty,
                    CellValue::Number(n) => FormulaResult::Number(n),
                    CellValue::String(s) => FormulaResult::Text(s),
                    CellValue::Boolean(b) => FormulaResult::Boolean(b),
                }
            };
            results.insert(address, value);
        }
    }
    Ok(Evaluation { formulas: results, formatted, percentage_styles, appearances, calculation_unavailable: is_1904,
        conditional_unavailable: (has_conditional && positions.is_none()) || unsupported_conditional })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_conditional_colors_and_preserves_cell_borders() {
        let source = include_bytes!("../tests/fixtures/conditional_appearance.xlsx");
        let result = evaluate(source).unwrap();
        assert!(!result.conditional_unavailable);
        let at = |row, column| result.appearances.get(&Address { sheet: 0, row, column }).unwrap();
        assert_eq!(at(0, 0).background, None);
        assert_eq!(at(1, 0).background, Some(0xffc7ce));
        assert_eq!(at(1, 0).foreground, Some(0x9c0006));
        assert!(at(1, 0).bold);
        assert_eq!(at(2, 0).background, Some(0xffc7ce));
        assert_eq!(at(0, 1).background, Some(0xf8696b));
        assert_eq!(at(2, 1).background, Some(0x63be7b));
        assert_eq!(at(0, 2).background, Some(0xffeb9c));
        assert_eq!(at(1, 2).background, None);
        assert_eq!(at(0, 4).background, Some(0xcceeff));
        assert_eq!(result.formatted[&Address { sheet: 0, row: 0, column: 3 }], "12.50%");
        assert_eq!(at(1, 0).borders, [
            Some(CellBorder { color: Some(0xff0000), width: 1 }), Some(CellBorder { color: Some(0x00aa00), width: 2 }),
            Some(CellBorder { color: Some(0x0000ff), width: 3 }), Some(CellBorder { color: Some(0x9c0006), width: 1 }),
        ]);
        let a = Address { sheet: 0, row: 0, column: 0 };
        let edited = crate::spreadsheet_xlsx::patch(source, &BTreeMap::from([(a, crate::spreadsheet_edit::CellInput::Number(250.0))])).unwrap();
        let updated = evaluate(&edited).unwrap();
        assert_eq!(updated.appearances.get(&a).unwrap().background, Some(0xffc7ce));
        assert_eq!(updated.formulas[&Address { sheet: 0, row: 2, column: 0 }], FormulaResult::Number(1000.0));
    }

    #[test]
    fn bounds_conditional_format_ranges_before_model_initialization() {
        let source = include_bytes!("../tests/fixtures/conditional_appearance.xlsx");
        let mut workbook = ironcalc::import::load_from_xlsx_bytes(source, "book", "en", "UTC").unwrap();
        workbook.worksheets[0].conditional_formatting[0].range = "A1:A1048576".to_string();
        assert!(conditional_positions(&workbook).is_err());
        workbook.worksheets[0].conditional_formatting[0].range = "A1:A3 C1:C2".to_string();
        let positions = conditional_positions(&workbook).unwrap();
        assert!(positions[0].contains(&(1, 1)) && positions[0].contains(&(2, 3)));
        let large = evaluate(include_bytes!("../tests/fixtures/conditional_oversized.xlsx")).unwrap();
        assert!(large.conditional_unavailable);
        assert_eq!(large.formulas[&Address { sheet: 0, row: 1, column: 0 }], FormulaResult::Number(200.0));
    }

    #[test]
    fn reads_cell_appearances_without_changing_workbook() {
        let source = include_bytes!("../tests/fixtures/cell_appearance.xlsx");
        let evaluated = evaluate(source).unwrap();
        let at = |row, column| evaluated.appearances.get(&Address { sheet: 0, row, column }).unwrap();
        let title = at(0, 0);
        assert_eq!(title.background, Some(0xffc7ce));
        assert_eq!(title.foreground, Some(0x9c0006));
        assert!(title.bold && title.italic && title.underline);
        assert_eq!(title.alignment, CellAlignment::Center);
        assert_eq!(at(0, 1).background, Some(0xc6efce));
        assert_eq!(at(0, 1).foreground, Some(0x006100));
        assert_eq!(at(0, 1).alignment, CellAlignment::Right);
        assert_eq!(at(1, 2).background, Some(0xffeb9c));
        assert_eq!(at(4, 4).background, Some(0xcceedd));
        assert!(at(3, 0).bold);
        assert_eq!(at(5, 0).background, None);
        assert_eq!(at(5, 0).foreground, None);
        assert_eq!(evaluated.formatted[&Address { sheet: 0, row: 0, column: 1 }], "1250.5");
        let a = Address { sheet: 0, row: 0, column: 0 };
        let b = Address { sheet: 0, row: 0, column: 1 };
        let edited = crate::spreadsheet_xlsx::patch(source, &BTreeMap::from([(a, crate::spreadsheet_edit::CellInput::Text("HP".into()))])).unwrap();
        let edited = crate::spreadsheet_xlsx::format_number_range(&edited, b, b, crate::spreadsheet_xlsx::NumberFormat::Decimal2).unwrap().snapshot;
        let after = evaluate(&edited).unwrap();
        assert_eq!(after.appearances.get(&a), evaluated.appearances.get(&a));
        assert_eq!(after.appearances.get(&b), evaluated.appearances.get(&b));
    }

    #[test]
    fn number_format_midpoints_round_away_from_zero_without_changing_values() {
        let mut cache = BTreeMap::new();
        for (value, format, expected) in [
            (0.125, "0.00", "0.13"), (-0.125, "0.00", "-0.13"),
            (1.005, "0.00", "1.01"), (2.675, "0.00", "2.68"),
            (0.00125, "0.00%", "0.13%"), (0.125, "0%", "13%"),
            (1234.5, "#,##0", "1,235"), (-1234.5, "#,##0;(#,##0)", "(1,235)"),
            (1.255, "0.00E+00", "1.26E+00"),
            (1.2, "0.00E+00", "1.20E+00"), (1.2, "0.00E-00", "1.20E00"),
            (-0.001, "0.00", "-0.00"), (-0.01, "0.00", "-0.01"),
        ] {
            let original = ironcalc::base::number_format::format_number(value, format, "en").text;
            let actual = corrected_number_text(value, format, &original, &mut cache).unwrap_or(original);
            assert_eq!(actual, expected, "{value} {format}");
        }
        assert_eq!(corrected_number_text(0.124_999_999_999, "0.00", "original", &mut cache), None);
        assert_eq!(corrected_number_text(0.125_000_000_001, "0.00", "original", &mut cache), None);
        assert_eq!(corrected_number_text(0.125, "[>0.12]0.00;0.0", "original", &mut cache), None);
        assert_eq!(corrected_number_text(0.125, "General", "original", &mut cache), None);
        for cents in 0..500 {
            let value = (2 * cents + 1) as f64 / 200.0;
            let expected = format!("{}.{:02}", (cents + 1) / 100, (cents + 1) % 100);
            for (value, expected) in [(value, expected.clone()), (-value, format!("-{expected}"))] {
                let original = ironcalc::base::number_format::format_number(value, "0.00", "en").text;
                let actual = corrected_number_text(value, "0.00", &original, &mut cache).unwrap_or(original);
                assert_eq!(actual, expected, "{value}");
            }
        }
        let source = include_bytes!("../tests/fixtures/number_formats.xlsx");
        let a = Address { sheet: 0, row: 0, column: 0 };
        let b = Address { sheet: 0, row: 0, column: 1 };
        let changed = crate::spreadsheet_xlsx::format_number_range(source, a, b, crate::spreadsheet_xlsx::NumberFormat::Decimal2).unwrap().snapshot;
        let evaluation = evaluate(&changed).unwrap();
        assert_eq!(evaluation.formatted[&a], "0.13");
        assert_eq!(evaluation.formatted[&b], "0.25");
        assert_eq!(evaluation.formulas[&b], FormulaResult::Number(0.25));
        let loaded = crate::spreadsheet::read_xlsx_snapshot(&changed).unwrap();
        assert!(loaded.sheets[0].cells.iter().any(|cell| cell.row == 0 && cell.column == 0 && cell.value == 0.125));
        let edited = crate::spreadsheet_xlsx::patch(&changed, &BTreeMap::from([(a, crate::spreadsheet_edit::CellInput::Number(0.0625))])).unwrap();
        let evaluation = evaluate(&edited).unwrap();
        assert_eq!(evaluation.formatted[&b], "0.13");
        assert_eq!(evaluation.formulas[&b], FormulaResult::Number(0.125));
    }

    #[test]
    fn overlapping_column_styles_keep_first_wins_and_scale_to_full_width() {
        let percent = FormatFlags { percentage: true, calendar_date: false };
        let date = FormatFlags { percentage: false, calendar_date: true };
        let mut columns = vec![(0, 2, percent), (1, 4, date), (16_383, 16_383, date)];
        columns.extend(std::iter::repeat_n((0, 16_383, percent), 16_384));
        let intervals = resolve_column_styles(&columns);
        assert_eq!(intervals, vec![(0, 2, percent), (3, 4, date), (5, 16_382, percent), (16_383, 16_383, date)]);
        let styles = PercentageStyles { sheets: vec![SheetPercentageStyles { columns: intervals, ..Default::default() }], is_1904: true };
        let at = |column| Address { sheet: 0, row: 0, column };
        assert_eq!(styles.get(&at(2)), Some(true));
        assert_eq!(styles.date_epoch(&at(3)), Some(true));
        assert_eq!(styles.get(&at(5)), Some(true));
        assert_eq!(styles.date_epoch(&at(16_383)), Some(true));
    }

    #[test]
    fn detects_calendar_formats_without_confusing_literals_or_durations() {
        for code in ["yyyy-mm-dd", "m/d/yy h:mm", "[$-409]d-mmm", "dd/mm;@"] { assert!(is_calendar_date_format(code), "{code}"); }
        for code in ["General", "0.00", "0.0%", "h:mm:ss", "[h]:mm:ss", "0 \"days\"", "0 \\d", "0;yyyy"] {
            assert!(!is_calendar_date_format(code), "{code}");
        }
    }

    #[test]
    fn blank_calendar_styles_survive_edits_clears_and_inheritance() {
        use crate::spreadsheet_edit::{CellInput, Session};
        let source = include_bytes!("../tests/fixtures/number_formats.xlsx");
        let evaluation = evaluate(source).unwrap();
        let at = |row, column| Address { sheet: 0, row, column };
        for address in [at(6, 1), at(6, 5), at(7, 0), at(7, 5)] {
            assert_eq!(evaluation.percentage_styles.date_epoch(&address), Some(false));
        }
        assert_eq!(evaluation.percentage_styles.date_epoch(&at(7, 6)), None);
        let mut session = Session::new(crate::spreadsheet::read_xlsx_snapshot(source).unwrap());
        let input = session.parse_input_at_with_styles(at(6, 1), "2026-02-03", &evaluation.percentage_styles).unwrap();
        assert_eq!(input, CellInput::Number(46056.0));
        session.set(at(6, 1), input).unwrap();
        session.paste_tsv_with_formats(at(6, 5), "2026-02-04\n2026-02-05", &evaluation.percentage_styles).unwrap();
        let input = session.parse_input_at_with_styles(at(7, 0), "2026-02-06", &evaluation.percentage_styles).unwrap();
        session.set(at(7, 0), input).unwrap();
        assert_eq!(session.parse_input_at_with_styles(at(7, 6), "2026-02-06", &evaluation.percentage_styles).unwrap(), CellInput::Text("2026-02-06".into()));
        let missing_styles = PercentageStyles::default();
        assert!(session.parse_input_at_with_styles(at(6, 1), "2026-02-03", &missing_styles).is_err());
        assert_eq!(session.parse_input_at_with_styles(at(6, 1), "'2026-02-03", &missing_styles).unwrap(), CellInput::Text("2026-02-03".into()));
        let updated = crate::spreadsheet_xlsx::patch(source, session.edits()).unwrap();
        let book = crate::spreadsheet::read_xlsx_snapshot(&updated).unwrap();
        for (address, expected) in [(at(6, 1), "2026-02-03"), (at(6, 5), "2026-02-04"), (at(7, 5), "2026-02-05"), (at(7, 0), "2026-02-06")] {
            let cell = book.sheets[0].cells.iter().find(|cell| (cell.row, cell.column) == (address.row, address.column)).unwrap();
            assert_eq!(crate::spreadsheet_edit::fallback_cell_text(cell), expected);
        }
        session.mark_saved(book);
        session.set(at(6, 1), CellInput::Clear).unwrap();
        let cleared = crate::spreadsheet_xlsx::patch(&updated, session.edits()).unwrap();
        session.mark_saved(crate::spreadsheet::read_xlsx_snapshot(&cleared).unwrap());
        let styles = evaluate(&cleared).unwrap().percentage_styles;
        let value = session.parse_input_at_with_styles(at(6, 1), "2027-01-02", &styles).unwrap();
        session.set(at(6, 1), value).unwrap();
        let restored = crate::spreadsheet_xlsx::patch(&cleared, session.edits()).unwrap();
        let book = crate::spreadsheet::read_xlsx_snapshot(&restored).unwrap();
        let cell = book.sheets[0].cells.iter().find(|cell| (cell.row, cell.column) == (6, 1)).unwrap();
        assert_eq!(crate::spreadsheet_edit::fallback_cell_text(cell), "2027-01-02");

        let source = include_bytes!("../tests/fixtures/dates_1904.xlsx");
        let styles = evaluate(source).unwrap().percentage_styles;
        assert_eq!(styles.date_epoch(&at(1, 4)), Some(true));
        let mut session = Session::new(crate::spreadsheet::read_xlsx_snapshot(source).unwrap());
        let value = session.parse_input_at_with_styles(at(1, 4), "2025-01-03", &styles).unwrap();
        assert_eq!(value, CellInput::Number(44198.0));
        session.set(at(1, 4), value).unwrap();
        let saved = crate::spreadsheet_xlsx::patch(source, session.edits()).unwrap();
        let book = crate::spreadsheet::read_xlsx_snapshot(&saved).unwrap();
        let cell = book.sheets[0].cells.iter().find(|cell| (cell.row, cell.column) == (1, 4)).unwrap();
        assert_eq!(crate::spreadsheet_edit::fallback_cell_text(cell), "2025-01-03");
    }

    #[test]
    fn refuses_incorrect_1904_calculation() {
        let source = include_bytes!("../tests/fixtures/dates_1904.xlsx");
        let evaluation = evaluate(source).unwrap();
        assert!(evaluation.calculation_unavailable && evaluation.formulas.is_empty());
        assert_eq!(evaluation.formatted[&Address { sheet: 0, row: 0, column: 0 }], "2025-01-02");
        assert_eq!(evaluation.formatted[&Address { sheet: 0, row: 0, column: 3 }], "25.0%");
        assert_eq!(evaluation.percentage_styles.get(&Address { sheet: 0, row: 0, column: 3 }), Some(true));
        assert!(calculate(source).unwrap_err().to_string().contains("1904 date system"));
        let book = crate::spreadsheet::read_xlsx_snapshot(source).unwrap();
        let mut session = crate::spreadsheet_edit::Session::new(book);
        let address = Address { sheet: 0, row: 0, column: 0 };
        let value = session.input(address).unwrap();
        assert_eq!(session.date_input_text(address, &value).as_deref(), Some("2025-01-02"));
        let updated = session.parse_input_at(address, "2025-01-03", None).unwrap();
        session.set(address, updated).unwrap();
        let bytes = crate::spreadsheet_xlsx::patch(source, session.edits()).unwrap();
        let book = crate::spreadsheet::read_xlsx_snapshot(&bytes).unwrap();
        let calamine::Data::DateTime(date) = book.sheets[0].cells[0].value else { panic!("date style was lost") };
        let (year, month, day, _, _, _, _) = date.to_ymd_hms_milli();
        assert_eq!((year, month, day), (2025, 1, 3));
    }

    #[test]
    fn evaluates_numeric_text_errors_and_cross_sheet_references() {
        let source = include_bytes!("../tests/fixtures/calculation.xlsx");
        let results = calculate(source).unwrap();
        let at = |sheet, row, column| &results[&Address { sheet, row, column }];
        assert_eq!(at(0, 0, 1), &FormulaResult::Number(30.0));
        assert_eq!(at(0, 1, 1), &FormulaResult::Text("high".into()));
        assert!(matches!(at(0, 2, 1), FormulaResult::Error(_)));
        assert_eq!(at(1, 0, 0), &FormulaResult::Number(60.0));
        assert_eq!(source.as_slice(), include_bytes!("../tests/fixtures/calculation.xlsx"));
    }

    #[test]
    fn recalculates_dependencies_after_value_change_without_touching_source() {
        use crate::spreadsheet_edit::CellInput;
        let source = include_bytes!("../tests/fixtures/calculation.xlsx");
        let changes = BTreeMap::from([(Address { sheet: 0, row: 0, column: 0 }, CellInput::Number(15.0))]);
        let updated = crate::spreadsheet_xlsx::patch(source, &changes).unwrap();
        let results = calculate(&updated).unwrap();
        assert_eq!(results[&Address { sheet: 0, row: 0, column: 1 }], FormulaResult::Number(35.0));
        assert_eq!(results[&Address { sheet: 1, row: 0, column: 0 }], FormulaResult::Number(70.0));
        let original = calculate(source).unwrap();
        assert_eq!(original[&Address { sheet: 0, row: 0, column: 1 }], FormulaResult::Number(30.0));
        assert!(calculate(b"not an xlsx").is_err());
        assert!(calculate(include_bytes!("../tests/fixtures/spreadsheet.xlsx")).is_err());
    }

    #[test]
    fn calculates_common_game_balance_formulas() {
        let results = calculate(include_bytes!("../tests/fixtures/game_balance.xlsx")).unwrap();
        for (offset, expected) in [300.0, 100.0, 120.0, 80.0, 2.0, 220.0, 80.0, 3.33, 99.0, 460.0].into_iter().enumerate() {
            assert_eq!(results.get(&Address { sheet: 0, row: offset as u32 + 1, column: 4 }), Some(&FormulaResult::Number(expected)), "formula row {}", offset + 2);
        }
    }

    #[test]
    fn formats_game_balance_numbers_dates_and_formula_results() {
        let evaluation = evaluate(include_bytes!("../tests/fixtures/number_formats.xlsx")).unwrap();
        let at = |row, column| evaluation.formatted.get(&Address { sheet: 0, row, column }).map(String::as_str);
        assert_eq!(at(0, 0), Some("12.50%"));
        assert_eq!(at(1, 0), Some("1,234.50"));
        assert_eq!(at(2, 0), Some("20%"));
        assert_eq!(at(3, 0), Some("2025-01-02"));
        assert_eq!(at(0, 1), Some("25.0%"));
        assert_eq!(evaluation.formulas[&Address { sheet: 0, row: 0, column: 1 }], FormulaResult::Number(0.25));
        assert_eq!(evaluation.percentage_styles.get(&Address { sheet: 0, row: 0, column: 0 }), Some(true));
        assert_eq!(evaluation.percentage_styles.get(&Address { sheet: 0, row: 1, column: 0 }), Some(false));
        assert_eq!(evaluation.percentage_styles.get(&Address { sheet: 0, row: 0, column: 1 }), Some(true));
        assert_eq!(evaluation.percentage_styles.get(&Address { sheet: 0, row: 3, column: 1 }), Some(true));
        assert_eq!(evaluation.percentage_styles.get(&Address { sheet: 0, row: 4, column: 2 }), Some(true));
        assert_eq!(evaluation.percentage_styles.get(&Address { sheet: 0, row: 5, column: 0 }), Some(true));
        assert_eq!(evaluation.percentage_styles.get(&Address { sheet: 0, row: 5, column: 3 }), Some(false));
        let changes = BTreeMap::from([(Address { sheet: 0, row: 0, column: 0 }, crate::spreadsheet_edit::CellInput::Number(0.5))]);
        let changed = crate::spreadsheet_xlsx::patch(include_bytes!("../tests/fixtures/number_formats.xlsx"), &changes).unwrap();
        let updated = evaluate(&changed).unwrap();
        assert_eq!(updated.formatted[&Address { sheet: 0, row: 0, column: 0 }], "50.00%");
        assert_eq!(updated.formatted[&Address { sheet: 0, row: 0, column: 1 }], "100.0%");
        let address = |row, column| Address { sheet: 0, row, column };
        let changes = BTreeMap::from([
            (address(3, 1), crate::spreadsheet_edit::CellInput::Number(0.5)),
            (address(4, 2), crate::spreadsheet_edit::CellInput::Number(0.25)),
            (address(5, 0), crate::spreadsheet_edit::CellInput::Number(0.75)),
            (address(5, 2), crate::spreadsheet_edit::CellInput::Number(0.4)),
            (address(5, 3), crate::spreadsheet_edit::CellInput::Number(2.0)),
        ]);
        let changed = crate::spreadsheet_xlsx::patch(include_bytes!("../tests/fixtures/number_formats.xlsx"), &changes).unwrap();
        let updated = evaluate(&changed).unwrap();
        for (row, column, expected) in [(3, 1, "50.00%"), (4, 2, "25%"), (5, 0, "75.0%"), (5, 2, "40.0%"), (5, 3, "2")] {
            assert_eq!(updated.formatted[&address(row, column)], expected);
        }
    }

    #[test]
    fn distinguishes_percentage_operators_from_literals() {
        for format in ["0%", "0.00%", "[Red]0%", "0.0%;(0.0%)"] { assert!(is_percentage_format(format), "{format}"); }
        for format in ["General", "0\"%\"", "0\\%", "0*%", "0_%", "[literal%]0", "0%[unterminated"] { assert!(!is_percentage_format(format), "{format}"); }
    }
}
