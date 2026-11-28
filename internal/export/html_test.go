package export

import (
	"encoding/base64"
	"strings"
	"testing"

	"github.com/neko233/markdownx/internal/doc"
)

func TestHTMLContentsReferencesAndFootnoteBacklinks(t *testing.T) {
	d := doc.ReadSource("[toc]\n\n# 章节\n\n## 章节\n\n# fn-1\n\n[**阅读**][ref] 脚注[^note] 再提[^note] [未知][x]\n\n![截图][pic]\n\n" +
		"[ref]: https://example.com/q?a=1&b=2 \"书名\"\n\n[pic]: <screenshots/window (2).png> \"说明\"\n\n" +
		"[^note]: 注脚 **内容**。\n\n    第二段。\n")
	page := Page(d, Options{})
	for _, want := range []string{
		`<nav class="toc" aria-label="目录">`,
		`<h1 id="章节">章节</h1>`, `<h2 id="章节-1">章节</h2>`, `<a href="#章节-1">章节</a>`,
		`<a href="https://example.com/q?a=1&amp;b=2" title="书名"><strong>阅读</strong></a>`,
		`<img src="screenshots/window (2).png" alt="截图" title="说明">`,
		`<sup><a id="fnref-1-1" href="#fn-1-1" role="doc-noteref">1</a></sup>`,
		`<sup><a id="fnref-1-2" href="#fn-1-1" role="doc-noteref">1</a></sup>`,
		`<li id="fn-1-1">`, `<p>注脚 <strong>内容</strong>。</p>`, `<p>第二段。</p>`,
		`href="#fnref-1-1" role="doc-backlink"`, `href="#fnref-1-2" role="doc-backlink"`, `[未知][x]`,
	} {
		if !strings.Contains(page, want) {
			t.Errorf("HTML missing %q", want)
		}
	}
	if strings.Contains(page, "[ref]:") || strings.Contains(page, "[^note]:") {
		t.Fatal("raw definitions leaked into the exported HTML")
	}
}

func TestDiagramsEmbedOfflineWithOriginalCodeFallback(t *testing.T) {
	d := doc.ReadSource("# 图表\n\n> ```mermaid\n> flowchart TD\n>   A[准备] --> B[发布]\n> ```\n\n```mermaid\nflowchart LR\n  A[invalid\n```\n")
	page := Page(d, Options{Diagrams: true})
	if !strings.Contains(page, `<blockquote>`+"\n"+`<figure class="diagram"><picture>`) ||
		!strings.Contains(page, `media="(prefers-color-scheme: dark)"`) || strings.Contains(page, "<script") ||
		!strings.Contains(page, `<pre><code class="language-mermaid">flowchart LR`) {
		t.Fatal("export did not embed the valid diagram and retain the invalid source")
	}
	_, rest, ok := strings.Cut(page, `<img src="data:image/svg+xml;base64,`)
	if !ok {
		t.Fatal("export did not embed the diagram")
	}
	encoded, _, _ := strings.Cut(rest, `"`)
	svg, err := base64.StdEncoding.DecodeString(encoded)
	if err != nil || !strings.Contains(string(svg), "发布") {
		t.Fatalf("diagram image payload: %s, %v", svg, err)
	}
	if disabled := Page(d, Options{}); strings.Contains(disabled, `<figure class="diagram"`) || !strings.Contains(disabled, "A[准备]") {
		t.Fatal("disabled diagrams changed code export")
	}
}

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
		`<h1 id="标题">标题</h1>`,
		`<h2 id="小节">小节</h2>`,
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
