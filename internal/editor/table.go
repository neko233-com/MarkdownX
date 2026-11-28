package editor

import "github.com/neko233/markdownx/internal/doc"

type TableCell struct {
	Block doc.BlockID
	Table *doc.Table
	Row   int
	Col   int
}

// TableCellAt finds the innermost table containing id.
func (c *Core) TableCellAt(id doc.BlockID) (TableCell, bool) {
	path, _ := c.Path(id)
	for i := len(path) - 1; i > 0; i-- {
		s := path[i]
		if s.IsCell {
			b := BlockAt(c.Doc.Blocks, path[:i])
			return TableCell{b.ID, b.Kind.(doc.TableKind).Table, s.R, s.C}, true
		}
	}
	return TableCell{}, false
}

// CellText returns the first editable block in the cell covering (row, col).
func CellText(t *doc.Table, row, col int) *doc.Block {
	row, col = t.Origin(row, col)
	for _, b := range t.Rows[row][col].Blocks {
		if b.Kind.HasText() {
			return b
		}
	}
	return nil
}

func (c *Core) focusCell(t *doc.Table, row, col int) {
	if b := CellText(t, row, col); b != nil {
		c.Sel = Caret(Pos{Block: b.ID})
	}
}

// MoveTableCell moves through editable cells in reading order.
func (c *Core) MoveTableCell(id doc.BlockID, delta int) bool {
	p, ok := c.TableCellAt(id)
	if !ok {
		return false
	}
	var ids []doc.BlockID
	current := -1
	p.Table.Cells(func(row, col int, _ *doc.TableCell) {
		if b := CellText(p.Table, row, col); b != nil {
			if row == p.Row && col == p.Col {
				current = len(ids)
			}
			ids = append(ids, b.ID)
		}
	})
	next := current + delta
	switch {
	case current < 0 || next < 0:
	case next == len(ids):
		c.insertTableRow(p, len(p.Table.Rows), 0)
	default:
		c.Sel = Caret(Pos{Block: ids[next]})
	}
	return true
}

func (c *Core) insertTableRow(p TableCell, index, column int) {
	t := p.Table
	align := make([]doc.Align, t.NCols())
	for i := range align {
		if b := CellText(t, 0, i); b != nil {
			align[i] = b.Align
		}
	}
	c.Checkpoint(EditOther)
	t.InsertRow(index)
	for i, cell := range t.Rows[index] {
		for _, b := range cell.Blocks {
			b.Align = align[i]
		}
	}
	c.focusCell(t, index, column)
	c.touch()
}

func (c *Core) InsertTableRow(id doc.BlockID) bool {
	p, ok := c.TableCellAt(id)
	if !ok {
		return false
	}
	c.insertTableRow(p, max(p.Table.HeaderRows, p.Row+1), p.Col)
	return true
}

func (c *Core) DeleteTableRow(id doc.BlockID) bool {
	p, ok := c.TableCellAt(id)
	if !ok {
		return false
	}
	t := p.Table
	if len(t.Rows) <= 1 {
		return true
	}
	c.Checkpoint(EditOther)
	oldHeaders := t.HeaderRows
	t.DeleteRows(p.Row, p.Row+1)
	if p.Row < oldHeaders {
		t.HeaderRows = max(1, oldHeaders-1)
	}
	c.focusCell(t, min(p.Row, len(t.Rows)-1), p.Col)
	c.touch()
	return true
}

func (c *Core) InsertTableColumn(id doc.BlockID) bool {
	p, ok := c.TableCellAt(id)
	if !ok {
		return false
	}
	c.Checkpoint(EditOther)
	p.Table.InsertCol(p.Col + 1)
	c.focusCell(p.Table, p.Row, p.Col+1)
	c.touch()
	return true
}

// MoveTableColumn moves the column and any columns that share merged cells.
func (c *Core) MoveTableColumn(id doc.BlockID, delta int) bool {
	p, ok := c.TableCellAt(id)
	if !ok || delta == 0 {
		return false
	}
	t := p.Table
	rangeAt := func(column int) doc.CellRange {
		return t.Expand(doc.CellRange{Bottom: len(t.Rows) - 1, Left: column, Right: column})
	}
	from := rangeAt(p.Col)
	target := from.Right + 1
	if delta < 0 {
		target = from.Left - 1
	}
	if target < 0 || target >= t.NCols() {
		return false
	}
	neighbor := rangeAt(target)
	to := neighbor.Right + 1
	if delta < 0 {
		to = neighbor.Left
	}
	c.Checkpoint(EditOther)
	t.MoveCols(from.Left, from.Right+1, to)
	c.touch()
	return true
}

func (c *Core) DeleteTableColumn(id doc.BlockID) bool {
	p, ok := c.TableCellAt(id)
	if !ok {
		return false
	}
	t := p.Table
	if t.NCols() <= 1 {
		return true
	}
	c.Checkpoint(EditOther)
	t.DeleteCols(p.Col, p.Col+1)
	c.focusCell(t, p.Row, min(p.Col, t.NCols()-1))
	c.touch()
	return true
}

func (c *Core) AlignTableColumn(id doc.BlockID, align doc.Align) {
	p, ok := c.TableCellAt(id)
	if !ok {
		return
	}
	c.Checkpoint(EditOther)
	p.Table.Cells(func(_, col int, cell *doc.TableCell) {
		if col == p.Col {
			for _, b := range cell.Blocks {
				b.Align = align
			}
		}
	})
	c.touch()
}

func (c *Core) ResizeTable(id doc.BlockID, rows, cols int) bool {
	p, ok := c.TableCellAt(id)
	if !ok || rows < 1 || rows > 512 || cols < 1 || cols > 64 || rows*cols > 4096 {
		return false
	}
	t := p.Table
	if rows == len(t.Rows) && cols == t.NCols() {
		return true
	}
	c.Checkpoint(EditOther)
	if cols < t.NCols() {
		t.DeleteCols(cols, t.NCols())
	}
	if rows < len(t.Rows) {
		t.DeleteRows(rows, len(t.Rows))
	}
	for t.NCols() < cols {
		t.InsertCol(t.NCols())
	}
	for len(t.Rows) < rows {
		t.InsertRow(len(t.Rows))
	}
	c.focusCell(t, min(p.Row, rows-1), min(p.Col, cols-1))
	c.touch()
	return true
}

// MoveParagraph moves the current block or table row one slot.
func (c *Core) MoveParagraph(id doc.BlockID, delta int) {
	if p, ok := c.TableCellAt(id); ok {
		c.moveTableRow(p, delta)
		return
	}
	list, at := c.siblingList(id)
	if list != nil && at+delta >= 0 && at+delta < len(*list) {
		c.Checkpoint(EditOther)
		b := *list
		b[at], b[at+delta] = b[at+delta], b[at]
		c.touch()
	}
}

func (c *Core) moveTableRow(p TableCell, delta int) {
	t := p.Table
	next := p.Row + delta
	if min(p.Row, next) < t.HeaderRows || next >= len(t.Rows) {
		return
	}
	top, bottom := min(p.Row, next), max(p.Row, next)
	for row := top; row <= bottom; row++ {
		for col := range t.Rows[row] {
			origin, oc := t.Origin(row, col)
			if origin != row || t.Rows[origin][oc].RowSpan > 1 {
				return
			}
		}
	}
	to := next
	if delta > 0 {
		to++
	}
	c.Checkpoint(EditOther)
	t.MoveRows(p.Row, p.Row+1, to)
	c.touch()
}
