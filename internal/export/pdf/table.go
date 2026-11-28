package pdf

import "github.com/neko233/markdownx/internal/doc"

const tablePadding = 5.0

type tableCell struct {
	x, width float64
	align    doc.Align
	lines    []line
}

type tableRow struct {
	cells   []tableCell
	lines   []float64
	colored bool
}

func (r tableRow) height(start, end int) float64 {
	h := 2 * tablePadding
	for _, v := range r.lines[start:end] {
		h += v
	}
	return h
}

func (w *writer) table(t *doc.Table, x, width float64) {
	if t == nil || t.NCols() == 0 || len(t.Rows) == 0 {
		return
	}
	// On a narrow sheet, continue very wide tables on separate pages.
	columnsPerPage := max(1, int(width/32))
	for start := 0; start < t.NCols(); start += columnsPerPage {
		if start > 0 {
			w.newSection()
		}
		end := min(start+columnsPerPage, t.NCols())
		w.tableColumns(t, x, width, start, end)
	}
	w.y += 9
}

func (w *writer) tableColumns(t *doc.Table, x, width float64, start, end int) {
	weights := w.tableWeights(t, start, end)
	boundaries := make([]float64, end-start+1)
	for i, weight := range weights {
		boundaries[i+1] = boundaries[i] + weight
	}
	total := boundaries[len(boundaries)-1]
	minimum := min(32, width/float64(end-start))
	flex := width - minimum*float64(end-start)
	for i := range boundaries {
		boundaries[i] = x + float64(i)*minimum + boundaries[i]*flex/total
	}
	rows := make([]tableRow, len(t.Rows))
	for i, cells := range t.Rows {
		rows[i] = w.tableRow(cells, boundaries, start, end, i < t.HeaderRows)
	}
	count := min(max(0, t.HeaderRows), len(rows))
	var headerHeight float64
	for _, row := range rows[:count] {
		headerHeight += row.height(0, len(row.lines))
	}
	if headerHeight > w.bodyHeight()/2 {
		count, headerHeight = 0, 0
	}
	paginate := func(inBody bool) {
		w.page()
		if inBody {
			for _, row := range rows[:count] {
				w.drawTableBand(row, 0, len(row.lines))
			}
		}
	}
	if count != 0 && len(rows) > count {
		w.keep(headerHeight + rows[count].height(0, 1))
	}
	for i, r := range rows {
		inBody := i >= count
		all := r.height(0, len(r.lines))
		if inBody && all <= w.bodyHeight()-headerHeight && all > w.available() && w.y > w.margin+headerHeight {
			paginate(true)
		}
		for from := 0; from < len(r.lines); {
			if r.height(from, from+1) > w.available() && w.y > w.margin {
				paginate(inBody)
			}
			to, height := from, 2*tablePadding
			for to < len(r.lines) && (to == from || height+r.lines[to] <= w.available()) {
				height += r.lines[to]
				to++
			}
			w.drawTableBand(r, from, to)
			from = to
		}
	}
}

func (w *writer) tableWeights(t *doc.Table, start, end int) []float64 {
	weights := make([]float64, end-start)
	var custom bool
	for i := start; i < end; i++ {
		weights[i-start] = max(1, float64(t.ColWidths[i]))
		custom = custom || t.ColWidths[i] != t.ColWidths[0]
	}
	if custom {
		return weights
	}
	// In a regular Markdown table, keep short columns compact. Allow users'
	// explicitly resized book columns to retain their proportions.
	for i := range weights {
		weights[i] = 1
	}
	for row, cells := range t.Rows {
		s := bodyStyle()
		s.size = 9.5
		if row < t.HeaderRows {
			s.weight = bold
		}
		for col := start; col < min(end, len(cells)); col++ {
			cell := cells[col]
			for _, b := range cell.Blocks {
				for _, line := range w.wrap(w.spans(b.Text, s), s, w.bodyWidth(), false) {
					ideal := max(0, line.width+2*tablePadding-32) / float64(max(1, cell.ColSpan))
					weights[col-start] = max(weights[col-start], ideal)
				}
			}
		}
	}
	return weights
}

func (w *writer) tableRow(cells []doc.TableCell, boundaries []float64, start, end int, header bool) tableRow {
	s := bodyStyle()
	s.size = 9.5
	if header {
		s.weight = bold
	}
	r := tableRow{colored: header, lines: []float64{s.size * 1.48}}
	for col := 0; col < end; col++ {
		var cell doc.TableCell
		if col < len(cells) {
			cell = cells[col]
		}
		last := min(end, col+max(1, int(cell.ColSpan)))
		if last <= start {
			continue
		}
		from, to := max(0, col-start), last-start
		x, width := boundaries[from], boundaries[to]-boundaries[from]
		box := tableCell{x: x, width: width}
		for i, b := range cell.Blocks {
			if i == 0 {
				box.align = b.Align
			}
			box.lines = append(box.lines, w.wrap(w.spans(b.Text, s), s, max(1, width-2*tablePadding), false)...)
		}
		for n, l := range box.lines {
			if n == len(r.lines) {
				r.lines = append(r.lines, l.height)
			} else {
				r.lines[n] = max(r.lines[n], l.height)
			}
		}
		r.cells = append(r.cells, box)
		col = last - 1
	}
	return r
}

func (w *writer) drawTableBand(r tableRow, from, to int) {
	h := r.height(from, to)
	for _, cell := range r.cells {
		if r.colored {
			w.fill(cell.x, w.y, cell.width, h, fillInk)
		}
		w.pdf.SetStrokeColor(ruleInk.r, ruleInk.g, ruleInk.b)
		w.pdf.SetLineWidth(0.55)
		w.pdf.RectFromUpperLeftWithStyle(cell.x, w.y, cell.width, h, "D")
		y := w.y + tablePadding
		for i := from; i < min(to, len(cell.lines)); i++ {
			l := cell.lines[i]
			x := cell.x + tablePadding
			remaining := max(0, cell.width-2*tablePadding-l.width)
			if cell.align == doc.AlignRight {
				x += remaining
			} else if cell.align == doc.AlignCenter {
				x += remaining / 2
			}
			w.drawLine(l, x, y)
			y += r.lines[i]
		}
	}
	w.y += h
}
