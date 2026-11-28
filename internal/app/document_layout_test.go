package app

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/neko233/markdownx/internal/doc"
)

func TestTableRowsShareEdgesAndWrapAtTheViewport(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("| 项 | 说明 |\n| --- | --- |\n| 工作区内的文档路径 | `https://example.org/manual/desktop/editor/formatting/table/layout.html` |\n| 备注 | " + strings.Repeat("此列包含可以换行的中文说明。", 6) + " |\n")
	a.path, a.hideSide = "排版.md", true
	tr := ui.NewTester(a.View, 920, 720)
	for _, width := range []int{920, 440, 920} {
		tr.SetSize(width, 720)
		tr.Frame()
		tr.Frame()
		var cells []ui.Rect
		for _, node := range tr.AccessAll() {
			if node.Role == ui.RoleColumnHeader || node.Role == ui.RoleCell {
				cells = append(cells, node.Bounds)
			}
		}
		if len(cells) != 6 {
			t.Fatalf("width %d: expected six visible cells, got %v", width, cells)
		}
		for i := 0; i < len(cells); i += 2 {
			l, r := cells[i], cells[i+1]
			if !nearDIP(l.X+l.W, r.X) || !nearDIP(l.Y, r.Y) || !nearDIP(l.H, r.H) ||
				!nearDIP(l.W, cells[0].W) || r.X+r.W > float32(width) {
				t.Fatalf("width %d: discontinuous grid at row %d: %+v", width, i/2, cells)
			}
			if i > 0 && !nearDIP(cells[i-2].Y+cells[i-2].H, l.Y) {
				t.Fatalf("gap between rows: %+v", cells)
			}
		}
		if cells[0].W >= cells[1].W*0.6 || cells[0].W < minCellWidth-1 || cells[2].H < cells[0].H+20 {
			t.Fatalf("width %d: expected a narrow label column, a wrapped row and readable labels: %+v", width, cells)
		}
		table := a.core.Doc.Blocks[0].Kind.(doc.TableKind).Table
		p := a.bufs[table.Rows[1][1].Blocks[0].ID]
		if !nearDIP(p.bounds.Y+p.bounds.H/2, cells[3].Y+cells[3].H/2) {
			t.Fatalf("short content was not centered within the full row: %+v, %+v", p.bounds, cells[3])
		}
	}
}

func nearDIP(a, b float32) bool { return a-b < 1.1 && b-a < 1.1 }

func TestListMarkerRemainsOnFirstWrappedLine(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("- " + strings.Repeat("一条需要折行的列表内容。", 12) + "\n- 下一项")
	a.path, a.hideSide = "列表.md", true
	tr := ui.NewTester(a.View, 700, 560)
	tr.Frame()
	tr.Frame()
	p := a.bufs[a.core.Doc.Blocks[0].ID]
	if p.bounds.H < documentFontSize*documentLeading*3 || p.font.Family != "Open Sans" {
		t.Fatalf("expected wrapped document text with the packaged font: %+v, %+v", p.bounds, p.font)
	}
	x := int(p.bounds.X - 13.5)
	y := int(p.bounds.Y + documentFontSize*documentLeading/2)
	im := tr.Image()
	first, middle := im.RGBAAt(x, y), im.RGBAAt(x, int(p.bounds.Y+p.bounds.H/2))
	if first.R >= 150 || middle.R < 180 {
		t.Fatalf("first line marker %v, paragraph midpoint %v", first, middle)
	}
}
