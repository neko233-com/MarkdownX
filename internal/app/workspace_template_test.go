package app

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"go.yaml.in/yaml/v3"
)

func TestFileTreeExpandsTemplateMetadataAndPlacesVisualCursor(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "周会")
	if err := os.Mkdir(root, 0o700); err != nil {
		t.Fatal(err)
	}
	templateSource := utf8BOM + strings.Join([]string{
		"---", "typora-template:", `  filename: "{{date:YYYY-MM-DD}} 站会"`,
		"  description: 团队会议记录", `title: "{{title}}"`, `notes: "{{clipboard}}"`,
		"tags: [例会, 研发]", "---", "", "# {{title}}", "", "在 {{time:HH:mm}} 开始。", "", "{{cursor}}", "",
	}, "\r\n")
	templatePath, err := saveTemplate("站会", templateSource)
	if err != nil {
		t.Fatal(err)
	}
	a := New(root)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	tr.SetClipboard("第一行 \"带引号\" # 明文\n第二行")
	workspaceTestMenu(t, tr, "周会", "从模板新建…")
	a.treeEdit.templateValues.now = time.Date(2026, 11, 27, 13, 5, 4, 0, time.Local)
	tr.Key(0, ui.KeyEnter)
	tr.Key(0, ui.KeyEnd)
	tr.Key(0, ui.KeyEnter)
	if a.treeEdit.name != "2026-11-27 站会" || !tr.HasText("团队会议记录") {
		t.Fatalf("the picker did not apply template metadata: %q", a.treeEdit.name)
	}
	saveWorkspaceSnapshot(t, tr, "21-workspace-template-metadata")
	tr.Key(0, ui.KeyTab)
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Type("纪要 #1.md")
	tr.Key(0, ui.KeyEnter)
	created := filepath.Join(root, "纪要 #1.md")
	data, err := os.ReadFile(created)
	if err != nil {
		t.Fatal(err)
	}
	md := string(data)
	if a.path != created || a.source != nil || a.dirty || !strings.HasPrefix(md, utf8BOM) ||
		strings.Contains(md, "typora-template") || !strings.HasSuffix(md, "# 纪要 #1\r\n\r\n在 13:05 开始。\r\n\r\n\r\n") ||
		strings.Contains(strings.ReplaceAll(md, "\r\n", ""), "\n") {
		t.Fatalf("created note differs from the requested template rendering: %q", md)
	}
	var fields struct {
		Title string
		Notes string
		Tags  []string
	}
	front := readTemplateFrontMatter(md)
	if front == nil || yaml.Unmarshal([]byte(md[front.start:front.end]), &fields) != nil ||
		fields.Title != "纪要 #1" || fields.Notes != tr.Clipboard() || strings.Join(fields.Tags, ",") != "例会,研发" {
		t.Fatalf("interpolation changed the YAML fields: %+v", fields)
	}
	tr.Type("写在下一段")
	if !a.dirty || a.source != nil || !strings.Contains(a.markdownSource(), "在 13:05 开始。\n\n写在下一段") {
		t.Fatalf("the cursor did not open a visual paragraph below the template: %q", a.markdownSource())
	}
	if original, err := os.ReadFile(templatePath); err != nil || string(original) != templateSource {
		t.Fatal("creating and editing a note modified the source template")
	}
}

func TestFileTreeTemplateStripsOnlyItsReservedSettings(t *testing.T) {
	const before = "---\r\n# 用户属性\r\ntags :   ['研究']\r\n"
	const after = "# 下一项注释\r\nstatus: 原样\r\n---\r\n\r\n正文。\r\n"
	tpl := prepareWorkspaceTemplate(template{Body: before + "typora-template:\r\n  filename: 文档\r\n" + after})
	got, err := tpl.render("新文档.md", &templateValues{})
	if err != nil || got.cursor != -1 || got.text != before+after {
		t.Fatalf("unexpected edit to otherwise unchanged YAML or body: %q, cursor %d, %v", got.text, got.cursor, err)
	}
}

func TestFileTreeTemplateFormatsTimeAndLeavesUnknownVariables(t *testing.T) {
	v := templateValues{now: time.Date(2026, 11, 7, 9, 5, 4, 0, time.Local), title: "报告"}
	source := `{{date}} {{time}} {{date:YYYY YY MMMM MMM MM M DD D dddd ddd HH H hh h mm m ss s A a [HH YYYY]}}`
	got := expandWorkspaceTemplate(source, &v, "\n", true)
	want := "2026-11-07 09:05 2026 26 November Nov 11 11 07 7 Saturday Sat 09 9 09 9 05 5 04 4 AM am HH YYYY"
	if got.text != want {
		t.Fatalf("date and time expanded to %q, want %q", got.text, want)
	}
	got = expandWorkspaceTemplate(`\{{title}} {{future}} {{cursor}}{{uuid}} {{uuid}}`, &v, "\n", true)
	const prefix = `{{title}} {{future}} `
	pattern := regexp.MustCompile(`^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$`)
	if got.text != prefix+v.uuid+" "+v.uuid || got.cursor != len(prefix) || !pattern.MatchString(v.uuid) {
		t.Fatalf("template escaping, cursor offset or UUID changed: %q, %d", got.text, got.cursor)
	}
}

func TestFileTreeTemplatePreservesFlowYAMLData(t *testing.T) {
	source := "---\n{typora-template: {filename: 流式}, labels: [评审, 设计], title: '{{title}}'}\n---\n\n留着这一行。  \n"
	tpl := prepareWorkspaceTemplate(template{Name: "备选", Body: source})
	v := templateValues{}
	if got := tpl.suggestedName(&v); got != "流式" {
		t.Fatalf("could not read the flow-map filename: %q", got)
	}
	got, err := tpl.render(`文档 #2 '引号'.md`, &v)
	if err != nil || strings.Contains(got.text, "typora-template") || !strings.HasSuffix(got.text, "留着这一行。  \n") {
		t.Fatalf("flow map was not materialized safely: %q, %v", got.text, err)
	}
	f := readTemplateFrontMatter(got.text)
	var fields map[string]any
	if f == nil || yaml.Unmarshal([]byte(got.text[f.start:f.end]), &fields) != nil || fields["title"] != `文档 #2 '引号'` {
		t.Fatalf("flow map changed the title value: %+v", fields)
	}
}
