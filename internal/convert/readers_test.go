package convert

import (
	"archive/zip"
	"bytes"
	"strings"
	"testing"
)

func TestHTML(t *testing.T) {
	src := `<html><head><title>x</title><style>p{}</style></head><body>
<h1>标题</h1>
<p>一段 <strong>粗</strong> 和 <em>斜</em>，还有 <a href="https://x.y">链接</a>。</p>
<ul><li>甲</li><li>乙</li></ul>
<ol><li>一</li><li>二</li></ol>
<blockquote><p>引用</p></blockquote>
<pre><code class="language-go">fmt.Println(1 &lt; 2)</code></pre>
<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table>
<p>换行<br>之后</p>
</body></html>`
	md, err := HTML([]byte(src))
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{
		"# 标题",
		"**粗**",
		"*斜*",
		"[链接](https://x.y)",
		"- 甲",
		"- 乙",
		"1. 一",
		"2. 二",
		"> 引用",
		"```go",
		"fmt.Println(1 < 2)",
		"| a | b |",
		"| 1 | 2 |",
	} {
		if !strings.Contains(md, want) {
			t.Errorf("missing %q in:\n%s", want, md)
		}
	}
	if strings.Contains(md, "p{}") {
		t.Errorf("style content leaked:\n%s", md)
	}
}

func TestRtf(t *testing.T) {
	src := `{\rtf1\ansi{\fonttbl{\f0 Times;}}
\f0 Hello \b bold\b0  and \i italic\i0 .\par
Second line.\par
\u20013?\u25991?`
	md, err := Rtf([]byte(src))
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"**bold**", "*italic*", "Second line.", "中文"} {
		if !strings.Contains(md, want) {
			t.Errorf("missing %q in:\n%s", want, md)
		}
	}
	if strings.Contains(md, "Times") {
		t.Errorf("font table leaked:\n%s", md)
	}
}

func TestRtfRejectsNonRtf(t *testing.T) {
	if _, err := Rtf([]byte("plain text")); err == nil {
		t.Fatal("expected an error")
	}
}

func TestOfd(t *testing.T) {
	content := `<?xml version="1.0"?>
<ofd:Page xmlns:ofd="x"><ofd:Content><ofd:Layer>
<ofd:TextObject><ofd:TextCode X="10" Y="10">第一段</ofd:TextCode></ofd:TextObject>
<ofd:TextObject><ofd:TextCode X="10" Y="30">第二段</ofd:TextCode></ofd:TextObject>
</ofd:Layer></ofd:Content></ofd:Page>`
	z := zipBytes(t, map[string]string{
		"OFD.xml":                        `<ofd:OFD xmlns:ofd="x"/>`,
		"Doc_0/Pages/Page_0/Content.xml": content,
		"Doc_0/Pages/Page_1/Content.xml": `<ofd:Page xmlns:ofd="x"><ofd:Content><ofd:Layer><ofd:TextObject><ofd:TextCode>第三段</ofd:TextCode></ofd:TextObject></ofd:Layer></ofd:Content></ofd:Page>`,
	})
	md, err := Ofd(z)
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"第一段", "第二段", "第三段"} {
		if !strings.Contains(md, want) {
			t.Errorf("missing %q in:\n%s", want, md)
		}
	}
}

func TestEpub(t *testing.T) {
	container := `<?xml version="1.0"?>
<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>`
	opf := `<?xml version="1.0"?>
<package><manifest>
<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
<item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/>
</manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>`
	z := zipBytes(t, map[string]string{
		"META-INF/container.xml": container,
		"OEBPS/content.opf":      opf,
		"OEBPS/ch1.xhtml":        `<html><body><h1>第一章</h1><p>正文一</p></body></html>`,
		"OEBPS/ch2.xhtml":        `<html><body><h1>第二章</h1><p>正文二</p></body></html>`,
	})
	md, err := Epub(z)
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"# 第一章", "正文一", "# 第二章", "正文二"} {
		if !strings.Contains(md, want) {
			t.Errorf("missing %q in:\n%s", want, md)
		}
	}
	if strings.Index(md, "第一章") > strings.Index(md, "第二章") {
		t.Error("chapters are out of spine order")
	}
}

func TestPDFRejectsNonPDF(t *testing.T) {
	if _, err := PDF([]byte("not a pdf")); err == nil {
		t.Fatal("expected an error")
	}
}

func TestPDFReadsLiteralStrings(t *testing.T) {
	// A content stream with two text draws, uncompressed.
	body := "BT /F1 12 Tf 72 720 Td (Hello) Tj 0 -14 Td (World) Tj ET"
	pdf := "%PDF-1.4\n1 0 obj\n<<>>\nstream\n" + body + "\nendstream\nendobj\n"
	out, err := PDF([]byte(pdf))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "Hello") || !strings.Contains(out, "World") {
		t.Fatalf("text = %q", out)
	}
}

func TestDetect(t *testing.T) {
	cases := map[string]Format{
		"a.md": FormatMarkdown, "a.markdown": FormatMarkdown, "a.txt": FormatMarkdown, "a.text": FormatMarkdown,
		"a.docx": FormatDocx, "a.xlsx": FormatXlsx, "a.epub": FormatEpub,
		"a.ofd": FormatOfd, "a.pdf": FormatPDF, "a.rtf": FormatRtf,
		"a.html": FormatHTML, "a.bin": FormatUnknown,
	}
	for name, want := range cases {
		if got := Detect(name); got != want {
			t.Errorf("Detect(%q) = %v, want %v", name, got, want)
		}
	}
}

func TestMarkdownDispatches(t *testing.T) {
	md, err := Markdown("note.md", []byte("# hi"))
	if err != nil || md != "# hi" {
		t.Fatalf("md = %q, err = %v", md, err)
	}
	if _, err := Markdown("x.bin", []byte("?")); err == nil {
		t.Fatal("unknown format should error")
	}
}

// zipBytes builds an in-memory ZIP archive.
func zipBytes(t *testing.T, files map[string]string) []byte {
	t.Helper()
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for name, body := range files {
		w, err := zw.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := w.Write([]byte(body)); err != nil {
			t.Fatal(err)
		}
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}
