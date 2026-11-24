package export

import (
	"strings"
	"testing"

	"github.com/neko233/markdownx/internal/doc"
)

func TestPageRendersBlocks(t *testing.T) {
	bold := doc.PlainRich("a ")
	bold.Push("bold", doc.Marks{Bold: true})
	link := doc.Marks{Link: strptr("https://x.y")}
	withLink := doc.PlainRich("see ")
	withLink.Push("link", link)

	d := &doc.Document{
		Title: doc.PlainRich("标题"),
		Blocks: []*doc.Block{
			doc.NewBlock(doc.HeadingKind{Level: 2}, doc.PlainRich("小节")),
			doc.NewBlock(doc.TextKind{}, bold),
			doc.NewBlock(doc.TextKind{}, withLink),
			doc.NewBlock(doc.BulletKind{}, doc.PlainRich("项")),
			doc.NewBlock(doc.CodeKind{Lang: "go"}, doc.PlainRich("x := 1")),
			doc.NewBlock(doc.QuoteKind{}, doc.RichText{}).WithChildren(doc.TextBlock("引用")),
			doc.NewBlock(doc.DividerKind{}, doc.RichText{}),
		},
	}
	page := Page(d, Options{Title: "标题"})
	for _, want := range []string{
		"<!doctype html>",
		"<title>标题</title>",
		"<h1>标题</h1>",
		"<h2>小节</h2>",
		"<strong>bold</strong>",
		`<a href="https://x.y">`,
		"<ul><li>项</li></ul>",
		`<pre><code class="language-go">x := 1</code></pre>`,
		"<blockquote>",
		"<hr>",
	} {
		if !strings.Contains(page, want) {
			t.Errorf("missing %q", want)
		}
	}
}

func TestPageEscapesText(t *testing.T) {
	d := &doc.Document{Blocks: []*doc.Block{doc.TextBlock("<script>alert(1)</script>")}}
	page := Page(d, Options{})
	if strings.Contains(page, "<script>alert(1)</script>") {
		t.Fatal("raw HTML from the document must be escaped")
	}
	if !strings.Contains(page, "&lt;script&gt;") {
		t.Fatal("expected escaped text")
	}
}

func TestPageTable(t *testing.T) {
	cell := func(s string) doc.TableCell {
		return doc.TableCell{Blocks: []*doc.Block{doc.TextBlock(s)}, ColSpan: 1, RowSpan: 1}
	}
	tbl := &doc.Table{HeaderRows: 1, Rows: [][]doc.TableCell{{cell("a"), cell("b")}, {cell("1"), cell("2")}}}
	d := &doc.Document{Blocks: []*doc.Block{doc.NewBlock(doc.TableKind{Table: tbl}, doc.RichText{})}}
	page := Page(d, Options{})
	for _, want := range []string{"<table>", "<th>a</th>", "<td>1</td>"} {
		if !strings.Contains(page, want) {
			t.Errorf("missing %q", want)
		}
	}
}

func strptr(s string) *string { return &s }
