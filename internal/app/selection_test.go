package app

import (
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func TestDocumentSelectionKeyboardAndNativeReplacement(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("**甲**😀\n\nsecond\n\nthird")
	one, two, three := a.core.Doc.Blocks[0], a.core.Doc.Blocks[1], a.core.Doc.Blocks[2]
	a.focusAt(editor.Caret(editor.Pos{Block: one.ID, Offset: len("甲😀")}))
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	tr.Key(ui.Shift, ui.KeyRight)
	tr.Key(ui.Shift, ui.KeyRight)
	if s := a.core.Sel; s.Head != (editor.Pos{Block: two.ID, Offset: 1}) || s.Anchor.Block != one.ID {
		t.Fatalf("Shift+Right across a paragraph: %+v", s)
	}
	tr.Type("新")
	if len(a.core.Doc.Blocks) != 2 || a.core.Doc.Blocks[0].Text.Text != "甲😀新econd" || a.core.Doc.Blocks[1].ID != three.ID {
		t.Fatalf("typing across paragraphs left %q", a.markdownSource())
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	if len(a.core.Doc.Blocks) != 3 || !a.core.Sel.AcrossBlocks() {
		t.Fatal("undo did not restore document range")
	}
	tr.Key(ui.Cmd, ui.KeyA)
	if !a.core.Sel.All {
		t.Fatal("Ctrl/Cmd+A did not select the document")
	}
	tr.Command("copy")
	data := tr.ClipboardData()
	html, err := data.Read(transfer.HTML)
	if err != nil || !strings.Contains(string(html), "<strong>甲</strong>") ||
		!strings.Contains(string(html), "<p>third</p>") || tr.Clipboard() != "甲😀\nsecond\nthird" {
		t.Fatalf("document clipboard: HTML %q, plain %q, error %v", html, tr.Clipboard(), err)
	}
	rtf, err := data.Read(richtext.RTF)
	bold := false
	for _, run := range richtext.ParseRTF(rtf).Runs(0, 1) {
		bold = run.Attributes.Bold()
	}
	if err != nil || !strings.Contains(string(rtf), `\u30002?`) || !strings.Contains(string(rtf), `\u-10179?\u-8704?`) ||
		!bold {
		t.Fatalf("document clipboard did not render Unicode RTF with bold text: %q (%v)", rtf, err)
	}
	tr.Key(ui.Cmd|ui.Shift, ui.KeyC)
	if got := tr.Clipboard(); got != "**甲**😀\n\nsecond\n\nthird\n" {
		t.Fatalf("copy as Markdown selected only the current input: %q", got)
	}
	tr.Command("cut")
	if !a.core.Doc.IsBlank() || a.focus != a.core.Doc.Blocks[0].ID {
		t.Fatal("cut did not leave an empty focused editor")
	}
	tr.Type("重写")
	if got := a.core.Doc.PlainText(); got != "重写" {
		t.Fatalf("typing after cut: %q", got)
	}
}

func TestDocumentSelectionPointerDragAndShiftClick(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("甲😀一\n\n内 *文*\n\n末三尾")
	one, mid, last := a.core.Doc.Blocks[0], a.core.Doc.Blocks[1], a.core.Doc.Blocks[2]
	tr := ui.NewTester(a.View, 950, 680)
	tr.Frame()
	point := func(id editor.Pos) ui.Point {
		t.Helper()
		p := a.bufs[id.Block]
		shape := p.layout(0, p.bounds.W)
		if shape == nil || p.bounds.W <= 0 {
			t.Fatalf("layout unavailable for %d", id.Block)
		}
		caret := shape.Caret(utf16Index(p.model.Text, id.Offset))
		return ui.Point{X: p.bounds.X + caret.X + 0.15, Y: p.bounds.Y + caret.Y + caret.H/2}
	}
	start := editor.Pos{Block: one.ID, Offset: len("甲😀")}
	end := editor.Pos{Block: last.ID, Offset: len("末三")}
	from, to := point(start), point(end)
	inside := point(editor.Pos{Block: mid.ID, Offset: len("内 ")})
	pixelX, pixelY := int(inside.X-1), int(inside.Y+4)
	before := tr.Image().RGBAAt(pixelX, pixelY)
	tr.Press(from.X, from.Y)
	tr.Move(to.X, to.Y)
	if tr.Image().RGBAAt(pixelX, pixelY) == before {
		t.Fatal("drag did not paint a selection in the intervening input")
	}
	tr.Release(to.X, to.Y)
	if s := a.core.Sel; s.Anchor != start || s.Head != end || a.focus != last.ID {
		t.Fatalf("pointer selection: %+v, focus %d, want %+v through %+v", s, a.focus, start, end)
	}
	tr.Key(ui.Cmd|ui.Shift, ui.KeyC)
	if got := tr.Clipboard(); got != "一\n\n内 *文*\n\n末三\n" {
		t.Fatalf("drag copied %q", got)
	}
	shifted := editor.Pos{Block: mid.ID, Offset: len("内 ")}
	over := point(shifted)
	tr.ClickAtWith(ui.Shift, over.X, over.Y)
	if s := a.core.Sel; s.Anchor != start || s.Head != shifted {
		t.Fatalf("Shift-click lost the original anchor: %+v", s)
	}
	tr.Key(0, ui.KeyLeft)
	if s := a.core.Sel; !s.Collapsed() || s.Head != start {
		t.Fatalf("Left did not collapse to the start: %+v", s)
	}
}

func TestDocumentSelectionExtendsWhileScrolling(t *testing.T) {
	isolateConfig(t)
	var lines strings.Builder
	for i := range 80 {
		fmt.Fprintf(&lines, "Paragraph %d\n\n", i)
	}
	a := newTestApp(lines.String())
	tr := ui.NewTester(a.View, 950, 300)
	tr.Frame()
	start := a.bufs[a.core.Doc.Blocks[0].ID].bounds
	x, y := start.X+2, start.Y+start.H/2
	bottom := a.editBounds.Y + a.editBounds.H - 2
	tr.Press(x, y)
	tr.Move(x, bottom)
	first := a.core.Sel.Head.Block
	for range 10 {
		tr.Advance(40 * time.Millisecond)
	}
	last := a.core.Sel.Head.Block
	if a.editScroll.Y <= 50 || last == first || !a.core.Sel.AcrossBlocks() {
		t.Fatalf("selection stopped at the viewport: scroll %.1f, head %d -> %d", a.editScroll.Y, first, last)
	}
	tr.Release(x, bottom)
	if a.drag != nil || a.focus != a.core.Sel.Head.Block {
		t.Fatal("release did not keep the offscreen range")
	}
}

func TestDocumentClipboardKeepsBlocksOnPaste(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("# 標題\n\n| Name | Value |\n| --- | --- |\n| A | *strong* |\n\n**tail**")
	tr := ui.NewTester(a.View, 950, 680)
	a.focusAt(editor.Caret(editor.Pos{Block: a.core.Doc.Blocks[0].ID}))
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Key(ui.Cmd, ui.KeyX)
	tr.Type("prepost")
	for range 4 {
		tr.Key(0, ui.KeyLeft)
	}
	tr.Key(ui.Cmd, ui.KeyV)
	blocks := a.core.Doc.Blocks
	if len(blocks) != 4 || blocks[0].Text.Text != "pre" || blocks[1].Text.Text != "標題" ||
		blocks[3].Text.Text != "tailpost" || !blocks[3].Text.MarksAt(2).Bold {
		t.Fatalf("structured paste left %q", a.markdownSource())
	}
	table, ok := blocks[2].Kind.(doc.TableKind)
	if !ok || table.Table.Rows[1][1].Blocks[0].Text.Text != "strong" || !table.Table.Rows[1][1].Blocks[0].Text.MarksAt(1).Italic {
		t.Fatal("paste lost the table or cell formatting")
	}
	tr.Command("undo")
	if len(a.core.Doc.Blocks) != 1 || a.core.Doc.PlainText() != "prepost" {
		t.Fatal("paste did not undo in one step")
	}
	tr.SetClipboardData(transfer.New(transfer.NewItem(transfer.Bytes(transfer.HTML,
		[]byte("<h2>External</h2><blockquote><p>quoted <em>text</em></p></blockquote>")))))
	tr.Command("paste")
	if got := a.markdownSource(); !strings.Contains(got, "## External") || !strings.Contains(got, "> quoted *text*") {
		t.Fatalf("HTML paste lost block semantics: %q", got)
	}
}
