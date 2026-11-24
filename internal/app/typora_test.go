package app

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func TestDetectMarkdownPrefix(t *testing.T) {
	cases := []struct {
		in    string
		want  string // block kind name, "" when nothing matches
		rest  string
		child bool
	}{
		{"# 标题", "HeadingKind:1", "标题", false},
		{"### 三级", "HeadingKind:3", "三级", false},
		{"####### 七级", "", "", false},
		{"#hashtag", "", "", false},
		{"- [ ] 待办", "TodoKind:false", "待办", false},
		{"- [x] 完成", "TodoKind:true", "完成", false},
		{"* [X] 完成", "TodoKind:true", "完成", false},
		{"- 项目", "BulletKind", "项目", false},
		{"* 项目", "BulletKind", "项目", false},
		{"+ 项目", "BulletKind", "项目", false},
		{"1. 第一", "OrderedKind", "第一", false},
		{"12. 第十二", "OrderedKind", "第十二", false},
		{"> 引用", "QuoteKind", "引用", true},
		{"```go", "CodeKind:go", "", false},
		{"普通段落", "", "", false},
		{"-没有空格", "", "", false},
	}
	for _, c := range cases {
		kind, rest, child, ok := detectMarkdownPrefix(c.in)
		if c.want == "" {
			if ok {
				t.Errorf("%q: 不应转换，得到 %T", c.in, kind)
			}
			continue
		}
		if !ok {
			t.Errorf("%q: 应转换", c.in)
			continue
		}
		if got := kindName(kind); got != c.want {
			t.Errorf("%q: kind = %s, want %s", c.in, got, c.want)
		}
		if rest != c.rest {
			t.Errorf("%q: rest = %q, want %q", c.in, rest, c.rest)
		}
		if child != c.child {
			t.Errorf("%q: child = %v, want %v", c.in, child, c.child)
		}
	}
}

// kindName renders a block kind compactly for the table above.
func kindName(k doc.BlockKind) string {
	switch v := k.(type) {
	case doc.HeadingKind:
		return "HeadingKind:" + itoa(int(v.Level))
	case doc.TodoKind:
		if v.Done {
			return "TodoKind:true"
		}
		return "TodoKind:false"
	case doc.BulletKind:
		return "BulletKind"
	case doc.OrderedKind:
		return "OrderedKind"
	case doc.QuoteKind:
		return "QuoteKind"
	case doc.CodeKind:
		return "CodeKind:" + v.Lang
	}
	return "other"
}

// TestTypingMarkersSwitchesBlocks types the markers through the real widgets.
func TestTypingMarkersSwitchesBlocks(t *testing.T) {
	cases := []struct {
		typed string
		check func(*doc.Block) bool
		name  string
	}{
		{"# ", func(b *doc.Block) bool {
			h, ok := b.Kind.(doc.HeadingKind)
			return ok && h.Level == 1
		}, "标题"},
		{"## ", func(b *doc.Block) bool {
			h, ok := b.Kind.(doc.HeadingKind)
			return ok && h.Level == 2
		}, "二级标题"},
		{"- ", func(b *doc.Block) bool { _, ok := b.Kind.(doc.BulletKind); return ok }, "项目符号"},
		{"1. ", func(b *doc.Block) bool { _, ok := b.Kind.(doc.OrderedKind); return ok }, "编号列表"},
		{"- [ ] ", func(b *doc.Block) bool { _, ok := b.Kind.(doc.TodoKind); return ok }, "待办"},
		{"```go", func(b *doc.Block) bool {
			c, ok := b.Kind.(doc.CodeKind)
			return ok && c.Lang == "go"
		}, "代码块"},
	}
	for _, c := range cases {
		a := New("")
		a.core = editor.New(&doc.Document{Blocks: []*doc.Block{doc.TextBlock("")}})
		a.ResetBuffers()
		id := a.core.Doc.Blocks[0].ID
		tr := ui.NewTester(a.View, 900, 600)
		a.focusReq = id
		tr.Frame()
		tr.Type(c.typed)
		tr.Frame()
		if got := a.core.Doc.Blocks[0]; !c.check(got) {
			t.Fatalf("%s: 输入 %q 后 kind = %T", c.name, c.typed, got.Kind)
		}
	}
}

// TestTypingQuoteWrapsTheText checks the quote marker moves the text into a
// child block, the shape the quote renderer expects.
func TestTypingQuoteWrapsTheText(t *testing.T) {
	a := New("")
	a.core = editor.New(&doc.Document{Blocks: []*doc.Block{doc.TextBlock("")}})
	a.ResetBuffers()
	id := a.core.Doc.Blocks[0].ID
	tr := ui.NewTester(a.View, 900, 600)
	a.focusReq = id
	tr.Frame()
	tr.Type("> ")
	tr.Frame()
	b := a.core.Doc.Blocks[0]
	if _, ok := b.Kind.(doc.QuoteKind); !ok {
		t.Fatalf("kind = %T", b.Kind)
	}
	if len(b.Children) != 1 {
		t.Fatalf("children = %d", len(b.Children))
	}
	if a.focus != b.Children[0].ID {
		t.Fatal("光标应落在引用的子块里")
	}
}

func TestEnterAfterHeadingStartsAParagraph(t *testing.T) {
	block := doc.NewBlock(doc.HeadingKind{Level: 2}, doc.PlainRich("标题"))
	d := &doc.Document{Blocks: []*doc.Block{block}}
	c := editor.New(d)
	c.Sel = editor.Caret(editor.Pos{Block: block.ID, Offset: len("标题")})
	c.Enter()
	if len(d.Blocks) != 2 {
		t.Fatalf("blocks = %d", len(d.Blocks))
	}
	if _, ok := d.Blocks[1].Kind.(doc.TextKind); !ok {
		t.Fatalf("Enter 后的新块是 %T，应为段落", d.Blocks[1].Kind)
	}
}

func TestEnterInAListContinuesIt(t *testing.T) {
	item := doc.NewBlock(doc.BulletKind{}, doc.PlainRich("第一项"))
	d := &doc.Document{Blocks: []*doc.Block{item}}
	c := editor.New(d)
	c.Sel = editor.Caret(editor.Pos{Block: item.ID, Offset: len("第一项")})
	c.Enter()
	if len(d.Blocks) != 2 {
		t.Fatalf("blocks = %d", len(d.Blocks))
	}
	if _, ok := d.Blocks[1].Kind.(doc.BulletKind); !ok {
		t.Fatalf("下一块是 %T，应继续列表", d.Blocks[1].Kind)
	}
	// Enter on the empty item leaves the list.
	c.Enter()
	if _, ok := d.Blocks[1].Kind.(doc.TextKind); !ok {
		t.Fatalf("空项回车后是 %T，应回到段落", d.Blocks[1].Kind)
	}
}

// TestTypingContinuesAfterEnter checks that Enter moves the caret into the
// block it creates, so the next keystrokes land there.
func TestTypingContinuesAfterEnter(t *testing.T) {
	a := New("")
	a.core = editor.New(&doc.Document{Blocks: []*doc.Block{doc.TextBlock("")}})
	a.ResetBuffers()
	id := a.core.Doc.Blocks[0].ID
	tr := ui.NewTester(a.View, 900, 600)
	a.focusReq = id
	tr.Frame()
	tr.Type("# 标题")
	tr.Frame()
	tr.Key(0, ui.KeyEnter)
	tr.Frame()
	tr.Type("- 项目")
	tr.Frame()

	blocks := a.core.Doc.Blocks
	if len(blocks) != 2 {
		t.Fatalf("blocks = %d, want 2", len(blocks))
	}
	if _, ok := blocks[0].Kind.(doc.HeadingKind); !ok {
		t.Fatalf("block 0 = %T", blocks[0].Kind)
	}
	if _, ok := blocks[1].Kind.(doc.BulletKind); !ok {
		t.Fatalf("block 1 = %T, want 列表", blocks[1].Kind)
	}
	if blocks[1].Text.Text != "项目" {
		t.Fatalf("block 1 text = %q", blocks[1].Text.Text)
	}
}

// TestTypingTodoInsideABullet checks "- " then "[ ] " makes a task item, which
// is how the marker arrives when it is typed rather than pasted.
func TestTypingTodoInsideABullet(t *testing.T) {
	a := New("")
	a.core = editor.New(&doc.Document{Blocks: []*doc.Block{doc.TextBlock("")}})
	a.ResetBuffers()
	id := a.core.Doc.Blocks[0].ID
	tr := ui.NewTester(a.View, 900, 600)
	a.focusReq = id
	tr.Frame()
	tr.Type("- ")
	tr.Frame()
	if _, ok := a.core.Doc.Blocks[0].Kind.(doc.BulletKind); !ok {
		t.Fatalf("kind = %T, want 列表", a.core.Doc.Blocks[0].Kind)
	}
	tr.Type("[ ] 待办")
	tr.Frame()
	got := a.core.Doc.Blocks[0]
	k, ok := got.Kind.(doc.TodoKind)
	if !ok {
		t.Fatalf("kind = %T, want 待办", got.Kind)
	}
	if k.Done {
		t.Fatal("Done 应为 false")
	}
	if got.Text.Text != "待办" {
		t.Fatalf("text = %q", got.Text.Text)
	}
}

func TestEnterLeavesAnEmptyQuote(t *testing.T) {
	quote := doc.NewBlock(doc.QuoteKind{}, doc.RichText{})
	quote.Children = []*doc.Block{doc.TextBlock("")}
	d := &doc.Document{Blocks: []*doc.Block{quote}}
	c := editor.New(d)
	c.Sel = editor.Caret(editor.Pos{Block: quote.Children[0].ID})
	c.Enter()
	if len(d.Blocks) != 1 {
		t.Fatalf("blocks = %d", len(d.Blocks))
	}
	if _, ok := d.Blocks[0].Kind.(doc.TextKind); !ok {
		t.Fatalf("空引用回车后是 %T，应为段落", d.Blocks[0].Kind)
	}

	// A quote with text keeps it and only lets the empty line go.
	first := doc.TextBlock("保留")
	quote2 := doc.NewBlock(doc.QuoteKind{}, doc.RichText{})
	emptyChild := doc.TextBlock("")
	quote2.Children = []*doc.Block{first, emptyChild}
	d2 := &doc.Document{Blocks: []*doc.Block{quote2}}
	c2 := editor.New(d2)
	c2.Sel = editor.Caret(editor.Pos{Block: emptyChild.ID})
	c2.Enter()
	if len(d2.Blocks) != 2 {
		t.Fatalf("blocks = %d", len(d2.Blocks))
	}
	if _, ok := d2.Blocks[0].Kind.(doc.QuoteKind); !ok {
		t.Fatalf("引用应保留，得到 %T", d2.Blocks[0].Kind)
	}
	if len(d2.Blocks[0].Children) != 1 {
		t.Fatalf("引用子块 = %d", len(d2.Blocks[0].Children))
	}
	if _, ok := d2.Blocks[1].Kind.(doc.TextKind); !ok {
		t.Fatalf("引用后的块是 %T", d2.Blocks[1].Kind)
	}
}

func TestInsertLinkMarksTheSelection(t *testing.T) {
	a := New("")
	a.core = editor.New(&doc.Document{Blocks: []*doc.Block{doc.TextBlock("示例文字")}})
	a.ResetBuffers()
	b := a.core.Doc.Blocks[0]
	a.focus = b.ID
	a.sel[b.ID] = [2]int{0, len("示例文字")}
	a.linkURL = "https://example.com"
	a.doInsertLink()
	md := doc.Export(a.core.Doc, doc.Options{}).Text
	if !strings.Contains(md, "[示例文字](https://example.com)") {
		t.Fatalf("Markdown 缺少链接:\n%s", md)
	}
}

func TestUndoReversesTyping(t *testing.T) {
	a := New("")
	a.core = editor.New(&doc.Document{Blocks: []*doc.Block{doc.TextBlock("")}})
	a.ResetBuffers()
	id := a.core.Doc.Blocks[0].ID
	tr := ui.NewTester(a.View, 900, 600)
	a.focusReq = id
	tr.Frame()
	tr.Type("hello")
	tr.Frame()
	if got := a.core.Doc.Blocks[0].Text.Text; got != "hello" {
		t.Fatalf("text = %q", got)
	}
	a.undo()
	if got := a.core.Doc.Blocks[0].Text.Text; got != "" {
		t.Fatalf("撤销后 text = %q, want 空", got)
	}
	a.redo()
	if got := a.core.Doc.Blocks[0].Text.Text; got != "hello" {
		t.Fatalf("重做后 text = %q", got)
	}
}

// TestBoldShortcutAppliesToTypedText checks Ctrl+B with nothing selected
// bolds the text typed next, the way Typora does.
func TestBoldShortcutAppliesToTypedText(t *testing.T) {
	a := New("")
	a.core = editor.New(&doc.Document{Blocks: []*doc.Block{doc.TextBlock("")}})
	a.ResetBuffers()
	id := a.core.Doc.Blocks[0].ID
	tr := ui.NewTester(a.View, 900, 600)
	a.focusReq = id
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyB)
	tr.Frame()
	tr.Type("加粗")
	tr.Frame()
	md := doc.Export(a.core.Doc, doc.Options{}).Text
	if !strings.Contains(md, "**加粗**") {
		t.Fatalf("Markdown 缺少加粗:\n%s", md)
	}
}

func TestShortcutHeadingAndParagraph(t *testing.T) {
	a := New("")
	a.core = editor.New(&doc.Document{Blocks: []*doc.Block{doc.TextBlock("快捷键")}})
	a.ResetBuffers()
	id := a.core.Doc.Blocks[0].ID
	tr := ui.NewTester(a.View, 900, 600)
	a.focusReq = id
	tr.Frame()
	tr.Key(ui.Cmd, ui.Key1)
	tr.Frame()
	if h, ok := a.core.Doc.Blocks[0].Kind.(doc.HeadingKind); !ok || h.Level != 1 {
		t.Fatalf("Ctrl+1 后 kind = %T", a.core.Doc.Blocks[0].Kind)
	}
	tr.Key(ui.Cmd, ui.Key0)
	tr.Frame()
	if _, ok := a.core.Doc.Blocks[0].Kind.(doc.TextKind); !ok {
		t.Fatalf("Ctrl+0 后 kind = %T", a.core.Doc.Blocks[0].Kind)
	}
}
