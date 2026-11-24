package convert

import (
	"archive/zip"
	"bytes"
	"encoding/xml"
	"fmt"
	"io"
	"strconv"
	"strings"
)

// Xlsx converts an .xlsx workbook into Markdown: one section per worksheet,
// each holding a GFM pipe table over the sheet's used range.
func Xlsx(data []byte) (string, error) {
	zr, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		return "", fmt.Errorf("xlsx: %w", err)
	}
	parts := make(map[string]*zip.File, len(zr.File))
	for _, f := range zr.File {
		parts[f.Name] = f
	}
	workbook, err := xlsxReadEntry(parts, "xl/workbook.xml")
	if err != nil {
		return "", fmt.Errorf("xlsx: %w", err)
	}
	names, err := xlsxSheetNames(workbook)
	if err != nil {
		return "", fmt.Errorf("xlsx: %w", err)
	}

	// 共享字符串表是可选部件，缺失时按空表处理。
	var shared []string
	if raw, err := xlsxReadEntry(parts, "xl/sharedStrings.xml"); err == nil {
		shared, err = xlsxSharedStrings(raw)
		if err != nil {
			return "", fmt.Errorf("xlsx: %w", err)
		}
	}

	sections := make([]string, 0, len(names))
	for i, name := range names {
		var rows [][]string
		// 工作表按顺序命名为 sheet1.xml、sheet2.xml ……
		path := fmt.Sprintf("xl/worksheets/sheet%d.xml", i+1)
		if raw, err := xlsxReadEntry(parts, path); err == nil {
			rows, err = xlsxSheetRows(raw, shared)
			if err != nil {
				return "", fmt.Errorf("xlsx: sheet %q: %w", name, err)
			}
		}
		sections = append(sections, xlsxSection(name, rows))
	}
	return strings.Join(sections, "\n"), nil
}

// xlsxReadEntry returns the content of a ZIP part.
func xlsxReadEntry(parts map[string]*zip.File, name string) ([]byte, error) {
	f, ok := parts[name]
	if !ok {
		return nil, fmt.Errorf("缺少 %s", name)
	}
	rc, err := f.Open()
	if err != nil {
		return nil, err
	}
	defer rc.Close()
	return io.ReadAll(rc)
}

// xlsxSheetNames lists worksheet names in workbook order.
func xlsxSheetNames(data []byte) ([]string, error) {
	var wb struct {
		Sheets struct {
			Sheet []struct {
				Name string `xml:"name,attr"`
			} `xml:"sheet"`
		} `xml:"sheets"`
	}
	if err := xml.Unmarshal(data, &wb); err != nil {
		return nil, err
	}
	if len(wb.Sheets.Sheet) == 0 {
		return nil, fmt.Errorf("工作簿中没有工作表")
	}
	names := make([]string, len(wb.Sheets.Sheet))
	for i, s := range wb.Sheets.Sheet {
		names[i] = s.Name
	}
	return names, nil
}

// xlsxSharedStrings reads xl/sharedStrings.xml, concatenating rich-text runs.
func xlsxSharedStrings(data []byte) ([]string, error) {
	dec := xml.NewDecoder(bytes.NewReader(data))
	var list []string
	var cur strings.Builder
	inSI, inT := false, false
	for {
		tok, err := dec.Token()
		if err == io.EOF {
			return list, nil
		}
		if err != nil {
			return nil, err
		}
		switch t := tok.(type) {
		case xml.StartElement:
			switch t.Name.Local {
			case "si":
				inSI, cur = true, strings.Builder{}
			case "t":
				inT = true
			}
		case xml.CharData:
			if inSI && inT {
				cur.Write(t)
			}
		case xml.EndElement:
			switch t.Name.Local {
			case "t":
				inT = false
			case "si":
				inSI = false
				list = append(list, cur.String())
			}
		}
	}
}

// xlsxSheetRows collects non-empty rows from a worksheet, placing each cell at
// the column decoded from its reference so gaps stay empty.
func xlsxSheetRows(data []byte, shared []string) ([][]string, error) {
	dec := xml.NewDecoder(bytes.NewReader(data))
	var rows [][]string
	var row map[int]string
	inRow, inV, inIS, inT := false, false, false, false
	nextCol := 0
	var ref, typ string
	var val, inline strings.Builder

	for {
		tok, err := dec.Token()
		if err == io.EOF {
			return rows, nil
		}
		if err != nil {
			return nil, err
		}
		switch t := tok.(type) {
		case xml.StartElement:
			switch t.Name.Local {
			case "row":
				row, inRow, nextCol = map[int]string{}, true, 0
			case "c":
				ref, typ = xlsxAttr(t, "r"), xlsxAttr(t, "t")
				val.Reset()
				inline.Reset()
				inV, inIS = false, false
			case "v":
				inV = true
			case "is":
				inIS = true
			case "t":
				inT = true
			}
		case xml.CharData:
			if inV {
				val.Write(t)
			} else if inIS && inT {
				inline.Write(t)
			}
		case xml.EndElement:
			switch t.Name.Local {
			case "v":
				inV = false
			case "t":
				inT = false
			case "is":
				inIS = false
			case "c":
				if inRow {
					col := xlsxColumn(ref, nextCol)
					row[col] = xlsxCell(typ, val.String(), inline.String(), shared)
					nextCol = col + 1
				}
			case "row":
				if inRow {
					if cells, ok := xlsxRowCells(row); ok {
						rows = append(rows, cells)
					}
					inRow = false
				}
			}
		}
	}
}

func xlsxAttr(e xml.StartElement, name string) string {
	for _, a := range e.Attr {
		if a.Name.Local == name {
			return a.Value
		}
	}
	return ""
}

// xlsxColumn decodes "A" / "AB12" into a zero-based column index; a missing
// reference falls back to the next free column.
func xlsxColumn(ref string, fallback int) int {
	col := 0
	for _, r := range ref {
		switch {
		case r >= 'A' && r <= 'Z':
			col = col*26 + int(r-'A') + 1
		case r >= 'a' && r <= 'z':
			col = col*26 + int(r-'a') + 1
		default:
			if col > 0 {
				return col - 1
			}
			return fallback
		}
	}
	if col > 0 {
		return col - 1
	}
	return fallback
}

// xlsxCell turns one cell into its display text and escapes it for Markdown.
func xlsxCell(typ, value, inline string, shared []string) string {
	s := value
	switch typ {
	case "s":
		// 越界索引视为空单元格。
		if i, err := strconv.Atoi(strings.TrimSpace(value)); err == nil && i >= 0 && i < len(shared) {
			s = shared[i]
		} else {
			s = ""
		}
	case "inlineStr":
		s = inline
	case "b":
		switch strings.TrimSpace(value) {
		case "1":
			s = "TRUE"
		case "0":
			s = "FALSE"
		}
	}
	return xlsxEscape(s)
}

// xlsxEscape keeps a cell on one table line.
func xlsxEscape(s string) string {
	s = strings.ReplaceAll(s, "|", `\|`)
	s = strings.ReplaceAll(s, "\r\n", "<br>")
	s = strings.ReplaceAll(s, "\n", "<br>")
	return strings.ReplaceAll(s, "\r", "<br>")
}

// xlsxRowCells drops an entirely empty row and trims its trailing empties.
func xlsxRowCells(row map[int]string) ([]string, bool) {
	last := -1
	for col, v := range row {
		if v != "" && col > last {
			last = col
		}
	}
	if last < 0 {
		return nil, false
	}
	cells := make([]string, last+1)
	for col, v := range row {
		if col <= last {
			cells[col] = v
		}
	}
	return cells, true
}

// xlsxSection renders one worksheet: a heading plus a padded pipe table whose
// first row is the header.
func xlsxSection(name string, rows [][]string) string {
	var b strings.Builder
	b.WriteString("## ")
	b.WriteString(name)
	b.WriteByte('\n')
	if len(rows) == 0 {
		return b.String()
	}
	width := 0
	for _, row := range rows {
		if len(row) > width {
			width = len(row)
		}
	}
	line := func(row []string) {
		b.WriteByte('|')
		for i := 0; i < width; i++ {
			b.WriteByte(' ')
			if i < len(row) {
				b.WriteString(row[i])
			}
			b.WriteString(" |")
		}
		b.WriteByte('\n')
	}
	line(rows[0])
	b.WriteByte('|')
	for i := 0; i < width; i++ {
		b.WriteString(" --- |")
	}
	b.WriteByte('\n')
	for _, row := range rows[1:] {
		line(row)
	}
	return b.String()
}
