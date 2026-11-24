package convert

import (
	"archive/zip"
	"bytes"
	"strings"
	"testing"
)

const docOpen = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body>`

const docClose = `</w:body></w:document>`

const relHeader = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">`

const relFooter = `</Relationships>`

func docxXML(body string) string { return docOpen + body + docClose }

// buildDocx writes the named parts into an in-memory ZIP.
func buildDocx(t *testing.T, files map[string]string) []byte {
	t.Helper()
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for name, content := range files {
		w, err := zw.Create(name)
		if err != nil {
			t.Fatalf("create %s: %v", name, err)
		}
		if _, err := w.Write([]byte(content)); err != nil {
			t.Fatalf("write %s: %v", name, err)
		}
	}
	if err := zw.Close(); err != nil {
		t.Fatalf("close zip: %v", err)
	}
	return buf.Bytes()
}

func TestDocx(t *testing.T) {
	cases := []struct {
		name      string
		body      string
		rels      string
		numbering string
		want      string
	}{
		{
			name: "headings",
			body: `<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>一级</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Heading 2"/></w:pPr><w:r><w:t>二级</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="标题 3"/></w:pPr><w:r><w:t>三级</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>标题</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Normal"/></w:pPr><w:r><w:t>正文</w:t></w:r></w:p>`,
			want: "# 一级\n\n## 二级\n\n### 三级\n\n# 标题\n\n正文\n",
		},
		{
			name: "bold-italic-underline-strike",
			body: `<w:p>
<w:r><w:rPr><w:b/></w:rPr><w:t>粗体</w:t></w:r>
<w:r><w:t xml:space="preserve"> 与 </w:t></w:r>
<w:r><w:rPr><w:i/></w:rPr><w:t>斜体</w:t></w:r>
<w:r><w:t xml:space="preserve"> 及 </w:t></w:r>
<w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>粗斜</w:t></w:r>
<w:r><w:t xml:space="preserve"> </w:t></w:r>
<w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>下划线</w:t></w:r>
<w:r><w:t xml:space="preserve"> </w:t></w:r>
<w:r><w:rPr><w:strike/></w:rPr><w:t>删除</w:t></w:r>
</w:p>`,
			want: "**粗体** 与 *斜体* 及 ***粗斜*** <u>下划线</u> ~~删除~~\n",
		},
		{
			name: "hyperlink",
			body: `<w:p><w:r><w:t xml:space="preserve">见 </w:t></w:r>
<w:hyperlink r:id="rId4"><w:r><w:rPr><w:b/></w:rPr><w:t>示例</w:t></w:r></w:hyperlink></w:p>`,
			rels: relHeader + `<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/a?x=1&amp;y=2" TargetMode="External"/>` + relFooter,
			want: "见 [**示例**](https://example.com/a?x=1&y=2)\n",
		},
		{
			name: "bullet-list",
			body: `<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>第一项</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>子项</w:t></w:r></w:p>`,
			want: "- 第一项\n  - 子项\n",
		},
		{
			name: "numbered-list",
			body: `<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>一</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>二</w:t></w:r></w:p>`,
			numbering: `<?xml version="1.0" encoding="UTF-8"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
</w:numbering>`,
			want: "1. 一\n1. 二\n",
		},
		{
			name: "table",
			body: `<w:tbl>
<w:tr>
<w:tc><w:p><w:r><w:t>姓名</w:t></w:r></w:p></w:tc>
<w:tc><w:p><w:r><w:t>值</w:t></w:r></w:p></w:tc>
</w:tr>
<w:tr>
<w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc>
<w:tc><w:p><w:r><w:t>x|y</w:t></w:r></w:p></w:tc>
</w:tr>
</w:tbl>`,
			want: "| 姓名 | 值 |\n| --- | --- |\n| a<br>b | x\\|y |\n",
		},
		{
			name: "xml-entities",
			body: `<w:p><w:r><w:t>A &amp; B &lt;tag&gt; &quot;q&quot; &#39;s&#39;</w:t></w:r></w:p>`,
			want: "A & B <tag> \"q\" 's'\n",
		},
		{
			name: "image-and-break",
			body: `<w:p><w:r><w:t>前</w:t><w:br/><w:t>后</w:t></w:r></w:p>
<w:p><w:r><w:drawing><wp:inline><wp:extent cx="1" cy="1"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><a:blip r:embed="rId9"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>`,
			rels: relHeader + `<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>` + relFooter,
			want: "前  \n后\n\n![](media/image1.png)\n",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			files := map[string]string{"word/document.xml": docxXML(tc.body)}
			if tc.rels != "" {
				files["word/_rels/document.xml.rels"] = tc.rels
			}
			if tc.numbering != "" {
				files["word/numbering.xml"] = tc.numbering
			}
			got, err := Docx(buildDocx(t, files))
			if err != nil {
				t.Fatalf("Docx: %v", err)
			}
			if got != tc.want {
				t.Errorf("Docx mismatch\n got: %q\nwant: %q", got, tc.want)
			}
		})
	}
}

func TestDocxDeletedRevisionSkipped(t *testing.T) {
	body := `<w:p><w:r><w:t>保留</w:t></w:r><w:del><w:r><w:delText>删除的旧文字</w:delText></w:r></w:del>` +
		`<w:ins><w:r><w:t>新增内容</w:t></w:r></w:ins></w:p>`
	got, err := Docx(buildDocx(t, map[string]string{"word/document.xml": docxXML(body)}))
	if err != nil {
		t.Fatalf("Docx: %v", err)
	}
	if want := "保留新增内容\n"; got != want {
		t.Errorf("Docx mismatch\n got: %q\nwant: %q", got, want)
	}
	if strings.Contains(got, "删除的旧文字") {
		t.Errorf("deleted text leaked into output: %q", got)
	}
}

func TestDocxNotAZip(t *testing.T) {
	if _, err := Docx([]byte("not a zip")); err == nil {
		t.Fatal("Docx accepted a non-ZIP input")
	}
}
