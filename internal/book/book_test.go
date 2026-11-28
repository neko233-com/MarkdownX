package book

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/neko233/markdownx/internal/doc"
)

func TestDocxXMLPreservesMarkdownReferences(t *testing.T) {
	const source = "[toc]\n\n[\"章节\"][Ref] 和 [^note]。\n\n[Ref]: #目的 \"说明\"\n\n[^note]: 第一段。\n\n    第二段。\n"
	before := doc.ReadSource(source)
	after := &doc.Document{Blocks: ParseDocxXML(ToDocxXML(before.Blocks))}
	if got, _ := after.WorkspaceMarkdown(); got != source {
		t.Fatalf("book changed Markdown reference source: %q", got)
	}
	if l, ok := after.References().Link("ref"); !ok || l.Destination != "#目的" {
		t.Fatalf("book lost the link definition: %+v", l)
	}
}

func TestParseRejectsBadBooks(t *testing.T) {
	cases := []string{
		`not json`,
		`{"format":"other","version":2,"chapters":[]}`,
		`{"format":"markdownx.book","version":9,"chapters":[{"id":"a","content":"<p>x</p>"}]}`,
		`{"format":"markdownx.book","version":2,"chapters":[]}`,
	}
	for _, c := range cases {
		if _, err := Parse([]byte(c)); err == nil {
			t.Errorf("expected an error for %s", c)
		}
	}
}

func TestParseAcceptsV1AndV2(t *testing.T) {
	for _, v := range []string{"1", "2"} {
		src := `{"format":"markdownx.book","version":` + v + `,"title":{"zh":"书"},
		"chapters":[{"id":"a","title":{"zh":"章"},"format":"docxxml","content":"<p>正文</p>"}]}`
		b, err := Parse([]byte(src))
		if err != nil {
			t.Fatalf("version %s: %v", v, err)
		}
		if len(b.Chapters) != 1 || b.Chapters[0].ID != "a" {
			t.Fatalf("version %s: %#v", v, b.Chapters)
		}
		if b.Reading.MaxWidth != 760 {
			t.Errorf("default max width = %v", b.Reading.MaxWidth)
		}
	}
}

func TestLocalizedAcceptsAString(t *testing.T) {
	var l Localized
	if err := l.UnmarshalJSON([]byte(`"纯文本"`)); err != nil {
		t.Fatal(err)
	}
	if l.Text() != "纯文本" {
		t.Fatalf("text = %q", l.Text())
	}
}

func TestDocxXMLParsesBlocks(t *testing.T) {
	src := `<h2>标题</h2>
<p>正文 <b>粗</b> 与 <latex>a^2</latex></p>
<ul><li>甲</li><li>乙</li></ul>
<checkbox done="true">做完的事</checkbox>
<blockquote><p>引用</p></blockquote>
<callout emoji="✳️" background-color="light-orange"><p>提示</p></callout>
<pre lang="go" caption="示例"><code>fmt.Println(1)</code></pre>
<table><thead><tr><th><p>列</p></th></tr></thead><tbody><tr><td><p>值</p></td></tr></tbody></table>
<grid><column width-ratio="0.6"><p>左</p></column><column width-ratio="0.4"><p>右</p></column></grid>
<interactive type="canvas" title="图" height="300"><![CDATA[{"kind":"quadratic"}]]></interactive>
<hr/>
<image src="assets/a.png" alt="图"/>`
	blocks := ParseDocxXML(src)
	if len(blocks) < 11 {
		t.Fatalf("blocks = %d, want at least 11", len(blocks))
	}
	var kinds []string
	for _, b := range blocks {
		kinds = append(kinds, kindName(b))
	}
	want := []string{"heading", "text", "bullet", "bullet", "todo", "quote", "callout", "code", "table", "grid", "interactive", "divider", "image"}
	for i, w := range want {
		if i >= len(kinds) {
			t.Fatalf("missing block %d (%s); got %v", i, w, kinds)
		}
		if kinds[i] != w {
			t.Fatalf("block %d = %s, want %s (all: %v)", i, kinds[i], w, kinds)
		}
	}

	h := blocks[0].Kind.(doc.HeadingKind)
	if h.Level != 2 {
		t.Fatalf("heading level = %d", h.Level)
	}
	if !strings.Contains(blocks[1].Text.Text, "粗") {
		t.Fatalf("text = %q", blocks[1].Text.Text)
	}
	if todo := blocks[4].Kind.(doc.TodoKind); !todo.Done {
		t.Fatal("checkbox should be done")
	}
	if code := blocks[7].Kind.(doc.CodeKind); code.Lang != "go" || code.Caption == nil || *code.Caption != "示例" {
		t.Fatalf("code = %#v", code)
	}
	if inter := blocks[10].Kind.(doc.InteractiveKindBlock).Interactive; inter.Height != 300 || !strings.Contains(inter.Source, "quadratic") {
		t.Fatalf("interactive = %#v", inter)
	}
}

func TestDocxXMLRoundTrips(t *testing.T) {
	src := `<h2>标题</h2><p>正文 <b>粗</b> <i>斜</i> <code>码</code> <a href="https://x.y">链接</a></p>
<ul><li>甲</li></ul><checkbox done="false">待办</checkbox>
<pre lang="go"><code>line1
line2</code></pre>
<table><thead><tr><th><p>H</p></th></tr></thead><tbody><tr><td><p>V</p></td></tr></tbody></table>
<interactive type="html" title="沙箱" height="200"><![CDATA[<b>x</b>]]></interactive>`
	first := ParseDocxXML(src)
	xml := ToDocxXML(first)
	second := ParseDocxXML(xml)
	if len(first) != len(second) {
		t.Fatalf("block count %d → %d\n%s", len(first), len(second), xml)
	}
	for i := range first {
		if kindName(first[i]) != kindName(second[i]) {
			t.Errorf("block %d: %s → %s", i, kindName(first[i]), kindName(second[i]))
		}
	}
	if plainOfBlocks(first) != plainOfBlocks(second) {
		t.Fatalf("text changed:\n%q\n→\n%q", plainOfBlocks(first), plainOfBlocks(second))
	}
	if !strings.Contains(xml, "<b>粗</b>") || !strings.Contains(xml, `<interactive type="html"`) {
		t.Fatalf("serialization lost markup:\n%s", xml)
	}
}

// TestRealBooks reads the sample books that ship with the repository.
func TestRealBooks(t *testing.T) {
	root := filepath.Join("..", "..", "examples")
	entries, err := os.ReadDir(root)
	if err != nil {
		t.Skip("examples folder not present")
	}
	checked := 0
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".mdxb") {
			continue
		}
		data, err := os.ReadFile(filepath.Join(root, e.Name()))
		if err != nil {
			t.Fatal(err)
		}
		b, err := Parse(data)
		if err != nil {
			t.Errorf("%s: %v", e.Name(), err)
			continue
		}
		d := b.Document()
		if len(d.Blocks) < 2 {
			t.Errorf("%s: document has %d blocks", e.Name(), len(d.Blocks))
		}
		for _, ch := range b.Chapters {
			blocks, err := ch.Blocks()
			if err != nil {
				t.Errorf("%s/%s: %v", e.Name(), ch.ID, err)
				continue
			}
			if len(blocks) == 0 {
				t.Errorf("%s/%s: no blocks", e.Name(), ch.ID)
			}
			// The chapter must survive a write/read cycle.
			again := ParseDocxXML(ToDocxXML(blocks))
			if len(again) == 0 {
				t.Errorf("%s/%s: lost everything on rewrite", e.Name(), ch.ID)
			}
		}
		checked++
	}
	if checked == 0 {
		t.Skip("no sample books found")
	}
	t.Logf("read %d books", checked)
}

func TestFromDocument(t *testing.T) {
	d := &doc.Document{
		Title: doc.PlainRich("标题"),
		Blocks: []*doc.Block{
			doc.NewBlock(doc.HeadingKind{Level: 1}, doc.PlainRich("章")),
			doc.TextBlock("正文"),
		},
	}
	b := FromDocument(d, "标题")
	if b.Format != Format || b.Version != Version {
		t.Fatalf("envelope = %#v", b)
	}
	if len(b.Chapters) != 1 {
		t.Fatalf("chapters = %d", len(b.Chapters))
	}
	data, err := b.Bytes()
	if err != nil {
		t.Fatal(err)
	}
	back, err := Parse(data)
	if err != nil {
		t.Fatal(err)
	}
	if back.Title.Text() != "标题" {
		t.Fatalf("title = %q", back.Title.Text())
	}
	blocks, err := back.Chapters[0].Blocks()
	if err != nil {
		t.Fatal(err)
	}
	if len(blocks) != 2 || kindName(blocks[1]) != "text" {
		t.Fatalf("blocks = %#v", blocks)
	}
}

func TestGuessMIME(t *testing.T) {
	cases := map[string]string{
		"a.png": "image/png", "a.JPG": "image/jpeg", "a.svg": "image/svg+xml",
		"a.pdf": "application/pdf", "a.mp4": "video/mp4", "a.unknown": "application/octet-stream",
	}
	for name, want := range cases {
		if got := GuessMIME(name); got != want {
			t.Errorf("GuessMIME(%q) = %q, want %q", name, got, want)
		}
	}
}

func TestUpdateFromDocumentKeepsEnvelopeAndChapters(t *testing.T) {
	src := `{"format":"markdownx.book","version":2,"title":{"zh":"书"},"authors":["某人"],
	"theme":{"accent":"#b45309"},
	"assets":{"assets/a.png":{"mime":"image/png","data":"AAAA"}},
	"chapters":[
	 {"id":"one","title":{"zh":"第一章"},"format":"docxxml","content":"<p>旧一</p>"},
	 {"id":"two","title":{"zh":"第二章"},"format":"markdown","content":"旧二"}
	]}`
	b, err := Parse([]byte(src))
	if err != nil {
		t.Fatal(err)
	}
	d := b.Document()
	// Append a third chapter and change the first one's body.
	d.Blocks = append(d.Blocks,
		doc.NewBlock(doc.HeadingKind{Level: 1}, doc.PlainRich("第三章")),
		doc.TextBlock("新三"),
	)
	b.UpdateFromDocument(d)

	if b.Title.Text() != "书" || len(b.Authors) != 1 || b.Theme.Accent != "#b45309" {
		t.Fatalf("envelope changed: %#v", b)
	}
	if _, ok := b.Assets["assets/a.png"]; !ok {
		t.Fatal("assets were dropped")
	}
	if len(b.Chapters) != 3 {
		t.Fatalf("chapters = %d, want 3", len(b.Chapters))
	}
	if b.Chapters[0].ID != "one" {
		t.Fatalf("first chapter id = %q", b.Chapters[0].ID)
	}
	if b.Chapters[1].Format != "markdown" {
		t.Fatalf("second chapter format = %q", b.Chapters[1].Format)
	}
	if b.Chapters[2].Title.Text() != "第三章" {
		t.Fatalf("third chapter title = %q", b.Chapters[2].Title.Text())
	}

	// The rewritten book must parse and flatten back to the same text.
	data, err := b.Bytes()
	if err != nil {
		t.Fatal(err)
	}
	again, err := Parse(data)
	if err != nil {
		t.Fatal(err)
	}
	if got := again.Document().PlainText(); !strings.Contains(got, "新三") {
		t.Fatalf("new chapter lost: %q", got)
	}
}

func kindName(b *doc.Block) string {
	switch b.Kind.(type) {
	case doc.TextKind:
		return "text"
	case doc.HeadingKind:
		return "heading"
	case doc.BulletKind:
		return "bullet"
	case doc.OrderedKind:
		return "ordered"
	case doc.TodoKind:
		return "todo"
	case doc.CodeKind:
		return "code"
	case doc.QuoteKind:
		return "quote"
	case doc.CalloutKind:
		return "callout"
	case doc.DividerKind:
		return "divider"
	case doc.ImageKind:
		return "image"
	case doc.TableKind:
		return "table"
	case doc.GridKind:
		return "grid"
	case doc.ColumnKind:
		return "column"
	case doc.SubPageListKind:
		return "subpagelist"
	case doc.CountdownKind:
		return "countdown"
	case doc.InteractiveKindBlock:
		return "interactive"
	case doc.FileKind:
		return "file"
	}
	return "unknown"
}

func plainOfBlocks(blocks []*doc.Block) string {
	var parts []string
	for _, b := range blocks {
		parts = append(parts, b.Text.PlainText())
	}
	return strings.Join(parts, "\n")
}
