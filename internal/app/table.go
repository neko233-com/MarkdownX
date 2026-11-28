package app

import (
	"strconv"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

const (
	cellPaddingX float32 = 13
	// Leave room for a short Chinese label before a wide table starts scrolling.
	minCellWidth = 2*cellPaddingX + 3*documentFontSize + 1
)

type columnWidth struct{ min, preferred float32 }

type cellWidthMeasurement struct {
	version uint64
	font    ui.Font
	width   columnWidth
}

// Measure the longest unbreakable run and the unwrapped line with the same
// shaper used by the editor. Results survive resizes and edits in other cells.
func (p *blockBuffer) measureCell(font ui.Font) columnWidth {
	v := p.native.Version()
	if cached := p.tableWidth; cached.font == font && cached.version == v {
		return cached.width
	}
	t := p.native.Text()
	var spans []ui.Span
	var inlineCodeWidth float32
	for _, r := range t.Runs(0, t.Len()) {
		s := ui.Span{Text: r.Text, Weight: r.Attributes.Weight, Italic: r.Attributes.Italic}
		if r.Attributes.Code {
			s.Font, s.Size, s.PaddingX = "monospace", font.Size*documentCodeScale, inlineCodePadding
			// Keep URLs, file names and identifiers together when allocating
			// columns. The editor can still wrap them in a narrow viewport.
			w, _ := ui.ShapeRichTextWith([]ui.Span{s}, font, ui.TextShape{}).Size()
			inlineCodeWidth = max(inlineCodeWidth, w)
		}
		if att := r.Attributes.Attachment; att != nil {
			s.ObjectWidth, s.ObjectHeight = att.Width, att.Height
		}
		spans = append(spans, s)
	}
	least, _ := ui.ShapeRichTextWith(spans, font, ui.TextShape{Width: 1, NoBreakWords: true}).Size()
	least = max(least, inlineCodeWidth)
	full, _ := ui.ShapeRichTextWith(spans, font, ui.TextShape{}).Size()
	width := columnWidth{least, max(full, least)}
	p.tableWidth = cellWidthMeasurement{v, font, width}
	return width
}

func (a *App) tableTracks(c *ui.Context, t *doc.Table, available float32) []ui.Track {
	cols := make([]columnWidth, t.NCols())
	for i := range cols {
		cols[i] = columnWidth{documentFontSize, documentFontSize}
	}
	for r, row := range t.Rows {
		for i, cell := range row {
			if cell.IsCovered() || i >= len(cols) {
				continue
			}
			font := ui.Font{Family: firstNonEmpty(documentFont(), c.Theme().Font), Size: documentFontSize, LineHeight: documentLeading}
			if r < t.HeaderRows || t.HeaderCol && i == 0 {
				font.Weight = 700
			}
			n := min(int(cell.ColSpan), len(cols)-i)
			for _, b := range cell.Blocks {
				if !b.Kind.HasText() {
					continue
				}
				w := a.blockBuffer(b).measureCell(font)
				for j := i; j < i+n; j++ {
					cols[j].min = max(cols[j].min, w.min/float32(n))
					cols[j].preferred = max(cols[j].preferred, w.preferred/float32(n))
				}
			}
		}
	}
	var least, preferred float32
	for i := range cols {
		pad := 2*cellPaddingX + 1
		cols[i].min += pad
		cols[i].preferred += pad
		least += cols[i].min
		preferred += cols[i].preferred
	}
	// Between the two intrinsic sizes, give each column its share of the
	// extra space it can use. Below the minimum, the editors wrap long words.
	fraction := min(1, max(0, (available-least)/max(1, preferred-least)))
	tracks := make([]ui.Track, len(cols))
	customWidths := false
	for _, w := range t.ColWidths {
		customWidths = customWidths || w != t.ColWidths[0]
	}
	for i, w := range cols {
		weight := w.min + fraction*(w.preferred-w.min)
		if customWidths {
			weight = max(1, t.ColWidths[i])
		}
		tracks[i] = ui.Fr(weight).Min(minCellWidth)
	}
	return tracks
}

func (a *App) table(c *ui.Context, id doc.BlockID, t *doc.Table) {
	if t == nil || t.NCols() == 0 {
		return
	}
	viewport := ui.ScrollHorizontal(c.Key(id)).FillWidth().MinWidth(0).AlignItems(ui.Start)
	w := viewport.Bounds().W
	if w == 0 {
		w = 800
		c.Invalidate()
	}
	w = max(w, float32(t.NCols())*minCellWidth)
	viewport.Children(func() {
		grid := ui.Grid(c).Width(w).Shrink(0).AlignItems(ui.Stretch).Role(ui.RoleTable)
		a.tableGrid(c, grid, id, t, w)
	})
}

func (a *App) tableGrid(c *ui.Context, grid ui.Element, id doc.BlockID, t *doc.Table, w float32) {
	theme := c.Theme()
	stripe := theme.Background.Mix(theme.Surface, 0.62)
	rule := theme.Background.Mix(theme.Border, 0.8)
	grid.ColumnTracks(a.tableTracks(c, t, w)...).Children(func() {
		for r, row := range t.Rows {
			for i, cell := range row {
				if cell.IsCovered() {
					continue
				}
				header := r < t.HeaderRows || t.HeaderCol && i == 0
				top, left := float32(0), float32(0)
				if r == 0 {
					top = 1
				}
				if i == 0 {
					left = 1
				}
				e := ui.Column(c).ColumnStart(i+1).RowStart(r+1).ColumnSpan(int(cell.ColSpan)).RowSpan(int(cell.RowSpan)).
					RowIndex(r+1).ColumnIndex(i+1).Role(ui.RoleCell).MinWidth(0).Justify(ui.Center).
					BorderColor(rule).BorderWidth(top, 1, 1, left).Padding(6, cellPaddingX-1, 6, cellPaddingX)
				if r < t.HeaderRows {
					e.Role(ui.RoleColumnHeader)
				} else if t.HeaderCol && i == 0 {
					e.Role(ui.RoleRowHeader)
				}
				if r < t.HeaderRows || (r-t.HeaderRows)%2 == 1 {
					e.Background(stripe)
				}
				if sel := a.tableRow; sel != nil && sel.table == id && r <= sel.row && sel.row < r+int(cell.RowSpan) {
					e.Background(theme.Selection)
				}
				e.Children(func() {
					for _, b := range cell.Blocks {
						if b.Kind.HasText() {
							a.blockInput(c, b, documentFontSize, header)
						}
					}
				})
			}
		}
	})
}

type tableRowSelection struct {
	table doc.BlockID
	row   int
	focus doc.BlockID
}

type tableSizeDialog struct {
	open    bool
	focus   bool
	editing doc.BlockID
	rows    string
	cols    string
	message string
}

func (a *App) selectTableCell(row bool) {
	var picked *tableRowSelection
	a.editFocused(func(b *doc.Block) bool {
		p, ok := a.core.TableCellAt(b.ID)
		if !ok {
			return false
		}
		a.core.Sel = editor.Selection{Anchor: editor.Pos{Block: b.ID}, Head: editor.Pos{Block: b.ID, Offset: b.Text.Len()}}
		if row {
			picked = &tableRowSelection{table: p.Block, row: p.Row, focus: b.ID}
		}
		return true
	})
	a.tableRow = picked
}

func (a *App) changeTable(edit func(*editor.Core, doc.BlockID) bool) {
	a.editFocused(func(b *doc.Block) bool { return edit(a.core, b.ID) })
}

func (a *App) alignTable(align doc.Align) {
	a.editFocused(func(b *doc.Block) bool {
		a.core.AlignTableColumn(b.ID, align)
		return true
	})
}

func (a *App) openTableSize() {
	b := a.focused()
	if b == nil {
		return
	}
	a.tableSize = tableSizeDialog{open: true, focus: true, rows: "3", cols: "3"}
	if p, ok := a.core.TableCellAt(b.ID); ok {
		a.tableSize.editing = b.ID
		a.tableSize.rows = itoa(len(p.Table.Rows))
		a.tableSize.cols = itoa(p.Table.NCols())
	}
}

func (a *App) applyTableSize() {
	d := &a.tableSize
	rows, _ := strconv.Atoi(d.rows)
	cols, _ := strconv.Atoi(d.cols)
	if rows < 1 || rows > 512 || cols < 1 || cols > 64 || rows*cols > 4096 {
		d.message = "输入有效的行数和列数，最多 512 行、64 列和 4096 个单元格。"
		return
	}
	if d.editing != 0 {
		a.syncAll()
		if !a.core.ResizeTable(d.editing, rows, cols) {
			d.message = "当前表格无法调整尺寸。"
			return
		}
		a.ResetBuffers()
		a.focusAt(a.core.Sel)
	} else {
		a.insertSizedTable(rows, cols)
	}
	d.open = false
}

func (a *App) tableDialog(c *ui.Context) {
	d := &a.tableSize
	if !d.open {
		return
	}
	t := c.Theme()
	ui.Modal(c, &d.open, func() {
		ui.Column(c).Width(320).Padding(20).Gap(10).Background(t.Surface).Radius(10).Children(func() {
			title := "插入表格"
			if d.editing != 0 {
				title = "调整表格"
			}
			ui.Text(c, title).FontSize(17).Bold()
			ui.Text(c, "行数（含表头）")
			rows := ui.TextInput(c.Key("rows"), &d.rows).Label("行数").OnSubmit(a.applyTableSize)
			if d.focus {
				rows.Focus()
				rows.SetTextSelection(0, len(d.rows))
				d.focus = false
			}
			ui.Text(c, "列数")
			ui.TextInput(c.Key("cols"), &d.cols).Label("列数").OnSubmit(a.applyTableSize)
			if d.message != "" {
				ui.Text(c, d.message).TextColor(t.Danger)
			}
			ui.Row(c).Gap(8).Children(func() {
				ui.Spacer(c).Grow(1)
				if ui.Button(c.Key("table-cancel"), "取消").Clicked() {
					d.open = false
				}
				if ui.PrimaryButton(c.Key("table-confirm"), "确定").Clicked() {
					a.applyTableSize()
				}
			})
		})
	})
}

func (a *App) tableMenu(m *ui.Menu, b *doc.Block, p *blockBuffer) {
	item := func(label string, action func()) {
		if m.Item(label).Chosen() {
			a.readSelection(b, p)
			action()
		}
	}
	m.EditItems()
	m.Separator()
	item("在下方插入行", func() { a.changeTable((*editor.Core).InsertTableRow) })
	item("删除当前行", func() { a.changeTable((*editor.Core).DeleteTableRow) })
	item("在右侧插入列", func() { a.changeTable((*editor.Core).InsertTableColumn) })
	item("删除当前列", func() { a.changeTable((*editor.Core).DeleteTableColumn) })
	m.Separator()
	item("本列左对齐", func() { a.alignTable(doc.AlignLeft) })
	item("本列居中", func() { a.alignTable(doc.AlignCenter) })
	item("本列右对齐", func() { a.alignTable(doc.AlignRight) })
	item("调整表格尺寸…", a.openTableSize)
}
