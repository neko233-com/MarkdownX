package app

import (
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown/parser"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func clickReference(t *testing.T, a *App, tr *ui.Tester, block doc.BlockID, i int, mods ui.Modifiers) {
	t.Helper()
	p := a.bufs[block]
	shape := p.layout(0, p.bounds.W)
	if shape == nil || len(shape.Objects()) <= i {
		t.Fatal("reference did not create a clickable inline")
	}
	r := shape.Objects()[i].Rect
	tr.ClickAtWith(mods, p.bounds.X+r.X+r.W/2, p.bounds.Y+r.Y+r.H/2)
}

func TestReferenceLinkOpensSourceAndFollowsEditedDefinition(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("查阅 [入口][route]。\n\n[route]: #旧标题 \"说明\"\n\n# 旧标题\n\n# 新标题\n")
	b, definition := a.core.Doc.Blocks[0], a.core.Doc.Blocks[1]
	tr := ui.NewTester(a.View, 910, 680)
	tr.Frame()
	clickReference(t, a, tr, b.ID, 0, 0)
	if a.inlineEdit == nil || !strings.Contains(a.bufs[b.ID].visible, "[入口][route]") {
		t.Fatal("click did not open the source of the reference")
	}
	tr.Key(ui.Ctrl, ui.KeyK)
	if a.focus != definition.ID {
		t.Fatal("Ctrl/Cmd+K did not open the shared definition")
	}
	tr.Type("#新标题")
	if got := a.markdownSource(); !strings.Contains(got, "[route]: #新标题 \"说明\"") {
		t.Fatalf("definition was not edited in place: %q", got)
	}
	tr.Advance(time.Second)
	clickReference(t, a, tr, b.ID, 0, ui.Cmd)
	if a.focus != a.core.Doc.Blocks[3].ID || a.inlineEdit != nil {
		t.Fatalf("the reference did not follow the updated definition: %q", a.status)
	}
}

func TestReferenceSelectionCopiesRichLinkAndAuthoredMarkdown(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("读 [资料][r] 和脚注[^n]。\n\n[r]: https://example.com \"说明\"\n\n[^n]: 细节\n")
	b := a.core.Doc.Blocks[0]
	a.focusAt(editor.Selection{Anchor: editor.Pos{Block: b.ID}, Head: editor.Pos{Block: b.ID, Offset: b.Text.Len()}})
	tr := ui.NewTester(a.View, 910, 680)
	tr.Frame()
	tr.Command("copy")
	h, err := tr.ClipboardData().Read(transfer.HTML)
	if err != nil || !strings.Contains(string(h), `<a href="https://example.com" title="说明">资料</a>`) ||
		!strings.Contains(string(h), `<p>细节</p>`) {
		t.Fatalf("the selection lost a definition: %q (%v)", h, err)
	}
	md, err := tr.ClipboardData().Read(markdownClipboard)
	if err != nil || strings.TrimSpace(string(md)) != "读 [资料][r] 和脚注[^n]。" {
		t.Fatalf("Markdown clipboard = %q (%v)", md, err)
	}
}

func TestFootnoteIsLiveAfterTypingItsDefinition(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("引文")
	b := a.core.Doc.Blocks[0]
	a.focusAt(editor.Caret(editor.Pos{Block: b.ID, Offset: b.Text.Len()}))
	tr := ui.NewTester(a.View, 900, 650)
	tr.Frame()
	tr.Type("[^a]")
	tr.Key(0, ui.KeyEnter)
	for _, ch := range "[^a]: 一段 **说明**" {
		tr.Type(string(ch))
	}
	tr.Key(0, ui.KeyEnter)
	note, ok := a.references().Footnote("a")
	if !ok || note.Number != 1 || (&doc.Document{Blocks: note.Body}).PlainText() != "一段 说明" {
		t.Fatalf("typed footnote was not recognized in %q", a.markdownSource())
	}
	p := a.bufs[b.ID]
	r := p.layout(0, p.bounds.W).Objects()[0].Rect
	tr.Move(p.bounds.X+r.X+r.W/2, p.bounds.Y+r.Y+r.H/2)
	tr.Advance(time.Second)
	if _, shown := tr.Find("一段 说明"); !shown {
		t.Fatal("hover did not open the footnote content")
	}
	clickReference(t, a, tr, b.ID, 0, ui.Cmd)
	if a.focus != note.Block || !strings.Contains(a.bufs[note.Block].visible, "[^a]: 一段 **说明**") {
		t.Fatal("Ctrl/Cmd-click did not jump to the editable footnote definition")
	}
	if err := tr.Click("返回脚注引用"); err != nil {
		t.Fatal(err)
	}
	if a.focus != b.ID || a.core.Sel.Head.Offset != len("引文") {
		t.Fatal("the return link did not go back to the exact footnote reference")
	}
}

func TestContentsTypedInBodyUpdatesAndNavigatesInPreview(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("目录位置\n\n# 开始\n\n> ## 子标题\n\n# 开始")
	first := a.core.Doc.Blocks[0]
	a.focusAt(editor.Selection{Anchor: editor.Pos{Block: first.ID}, Head: editor.Pos{Block: first.ID, Offset: first.Text.Len()}})
	tr := ui.NewTester(a.View, 920, 680)
	tr.Frame()
	tr.Type("[TOC]")
	tr.Key(0, ui.KeyEnter)
	if _, ok := a.core.Doc.Blocks[0].Kind.(doc.TOCKind); !ok {
		t.Fatalf("Enter did not create the TOC: %q", a.markdownSource())
	}
	if err := tr.Click("目录：子标题"); err != nil {
		t.Fatal(err)
	}
	if b := a.focused(); b.Text.Text != "子标题" {
		t.Fatalf("TOC focused %q", b.Text.Text)
	}
	tr.Type("新")
	if err := tr.Click("目录：新子标题"); err != nil {
		t.Fatal(err)
	}
	a.setMode(ModePreview)
	tr.Frame()
	if err := tr.Click("目录：新子标题"); err != nil {
		t.Fatal(err)
	}
	if a.md.AnchorVersion == 0 {
		t.Fatal("preview TOC did not request a heading scroll")
	}
	parsed := parser.Parse(a.markdownSource())
	target := a.md.AnchorBlock(parsed, a.md.Anchor)
	if target <= 0 || strings.TrimSpace(parsed.Lines(target)) != "新子标题" {
		t.Fatal("preview resolved the wrong heading in the quotation")
	}
}
