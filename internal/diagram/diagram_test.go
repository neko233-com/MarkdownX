package diagram

import (
	"bytes"
	"encoding/xml"
	"io"
	"strings"
	"testing"
)

func TestFencedDiagramTypes(t *testing.T) {
	for _, item := range []struct{ name, fence, source, label string }{
		{"flowchart", "mermaid", "flowchart LR\n  A[开始] --> B{检查}\n  B -->|通过| C([完成])\n  B -->|返回| A", "通过"},
		{"sequence", "mermaid", "sequenceDiagram\n  participant A as 编辑器\n  participant B as 文件\n  loop 自动保存\n    A->>+B: 写入\n    B-->>-A: 保存成功\n  end", "保存成功"},
		{"gantt", "mermaid", "gantt\n  title 文档计划\n  dateFormat YYYY-MM-DD\n  section 校对\n    初稿 :done, a, 2026-04-02, 2d\n    复核 :active, b, after a, 3d", "复核"},
		{"class", "mermaid", "classDiagram\n  class Document {\n    +String title\n  }\n  Document *-- Section", "Section"},
		{"state", "mermaid", "stateDiagram\n  [*] --> Editing\n  Editing --> Saved\n  Saved --> [*]", "Saved"},
		{"pie", "mermaid", "pie\n  title 资料分类\n  \"文档\" : 8\n  \"图片\" : 4", "资料分类"},
		{"ER", "mermaid", "erDiagram\n  NOTE ||--o{ TAG : labels\n  NOTE {\n    string title\n  }", "title"},
		{"requirement", "mermaid", "requirementDiagram\n  requirement visual {\n    id: 12\n    text: Review\n    risk: medium\n    verifymethod: inspection\n  }\n  element output {\n    type: svg\n  }\n  output - satisfies -> visual", "visual"},
		{"quadrant", "mermaid", "quadrantChart\n  title 可读性与完成度\n  x-axis Low --> High\n  y-axis Draft --> Final\n  Example: [0.7, 0.8]", "Example"},
		{"sankey", "mermaid", "sankey-beta\n  Draft,Review,5\n  Review,Release,4\n  Review,Revision,1", "Revision"},
		{"xy chart", "mermaid", "xychart-beta\n  title 文档统计\n  x-axis [Mon, Tue, Wed]\n  y-axis Pages 0 --> 10\n  bar [6, 9, 4]", "文档统计"},
		{"git graph", "mermaid", "gitGraph\n  commit id: \"initial\"\n  branch edit\n  checkout edit\n  commit id: \"review\"\n  checkout main\n  merge edit", "review"},
		{"mindmap", "mermaid", "mindmap\n  root((计划))\n    写作\n      校稿\n    发布", "校稿"},
		{"timeline", "mermaid", "timeline\n  title 交付计划\n  section 编辑\n    4 月 : 初稿 : 校对", "校对"},
		{"c4", "mermaid", "C4Context\n  Person(author, \"作者\", \"编写文档\")\n  System(editor, \"MarkdownX\", \"本地编辑器\")\n  Rel(author, editor, \"使用\")", "MarkdownX"},
		{"legacy flow", "flow", "st=>start: 开始\ncheck=>condition: 检查 (A[1])\nwrite=>operation: 写入\nstop=>end: 完成\n\nst->check\ncheck(yes,right)->write->stop\ncheck(no)->st", "检查 (A[1])"},
		{"legacy sequence", "sequence", "Title: 保存流程\nEditor->Disk: 生成草稿\nNote right of Disk: 本地保存\nDisk-->Editor: 完成", "本地保存"},
	} {
		t.Run(item.name, func(t *testing.T) {
			svg, err := Render(item.fence, item.source, "sans-serif", false)
			if err != nil {
				t.Fatal(err)
			}
			decoder := xml.NewDecoder(bytes.NewReader(svg))
			var labels strings.Builder
			for {
				token, err := decoder.Token()
				if err == io.EOF {
					break
				}
				if err != nil {
					t.Fatal(err)
				}
				if text, ok := token.(xml.CharData); ok {
					labels.Write(text)
				}
			}
			if !bytes.Contains(svg, []byte("<svg")) || !strings.Contains(labels.String(), item.label) {
				t.Fatalf("SVG lost label %q, got %q", item.label, labels.String())
			}
		})
	}
}

func TestDiagramStatusAndLocalConfig(t *testing.T) {
	source := "flowchart TD\n  A[输入] --> B[输出]"
	plain, err := Render("mermaid", source, "sans-serif", false)
	if err != nil {
		t.Fatal(err)
	}
	dark, err := Render("mermaid", source, "sans-serif", true)
	if err != nil || bytes.Equal(plain, dark) {
		t.Fatalf("dark style did not change: %v", err)
	}
	configured := `%%{init: {'theme':'dark', 'flowchart':{'nodeSpacing':40}}}%%` + "\n" + source
	local, err := Render("mermaid", configured, "sans-serif", false)
	if err != nil || !bytes.Equal(dark, local) {
		t.Fatalf("diagram-local theme did not apply: %v", err)
	}
	if _, err := Render("mermaid", "flowchart LR\n  A[unterminated", "", false); err == nil || !strings.Contains(err.Error(), "语法错误") {
		t.Fatalf("invalid diagram had no syntax diagnostic: %v", err)
	}
	if _, err := Render("flow", "a=>start\na->unknown", "", false); err == nil || !strings.Contains(err.Error(), "第 2 行") {
		t.Fatalf("flow error lost the source line: %v", err)
	}
}

func TestFlowchartArrowsReachNonRectangularNodes(t *testing.T) {
	data := `<svg>
 <path d="M0,10 L20,10" fill="none" stroke="black" stroke-width="1"/>
 <path d="M20,30 L0,30" fill="none" stroke="black" stroke-width="1"/>
 <path d="M150,6 L120,6" fill="none" stroke="black" stroke-width="1"/>
 <polygon points="40,0 60,20 40,40 20,20"/>
 <circle cx="100" cy="10" r="20"/>
</svg>`
	got := string(attachFlowchartEdges([]byte(data)))
	for _, want := range []string{"M0.00,10.00 L30.00,10.00", "M30.00,30.00 L0.00,30.00", "L119.60,6.00"} {
		if !strings.Contains(got, want) {
			t.Fatalf("edge did not meet the outline (%s): %s", want, got)
		}
	}
}
