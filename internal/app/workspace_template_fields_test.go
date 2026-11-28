package app

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

func workspaceTemplateFieldFixture(t *testing.T, body string) (*App, *ui.Tester) {
	t.Helper()
	root := filepath.Join(isolateConfig(t), "填空")
	if err := os.Mkdir(root, 0o700); err != nil {
		t.Fatal(err)
	}
	if _, err := saveTemplate("记录", body); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	workspaceTestMenu(t, tr, "填空", "从模板新建…")
	tr.Key(0, ui.KeyEnter)
	tr.Key(0, ui.KeyEnd)
	tr.Key(0, ui.KeyEnter)
	tr.Key(0, ui.KeyTab)
	tr.Key(0, ui.KeyEnter)
	if a.path != filepath.Join(root, "记录.md") || a.templateSession == nil || a.dirty {
		t.Fatalf("the directory tree did not start a template session: %q (%s)", a.path, a.status)
	}
	return a, tr
}

func templateVisualSelection(a *App) string {
	sel := a.core.Sel
	if sel.Anchor.Block == sel.Head.Block {
		if text := a.core.TextOf(sel.Head.Block); text != nil {
			return text.Text[min(sel.Head.Offset, sel.Anchor.Offset):max(sel.Head.Offset, sel.Anchor.Offset)]
		}
	}
	return ""
}

func TestFileTreeTemplateFieldsMirrorInVisualEditorAndTabInNumberOrder(t *testing.T) {
	a, tr := workspaceTemplateFieldFixture(t, utf8BOM+"# {{2:会议}}\r\n\r\n姓名 {{1:人员}}\r\n\r\n确认 {{1}}\r\n\r\n{{cursor}}\r\n")
	if a.source != nil || templateVisualSelection(a) != "人员" {
		t.Fatalf("the first numerical field was not selected visually: %q", templateVisualSelection(a))
	}
	tr.Type("田小明😀")
	if strings.Count(a.markdownSource(), "田小明😀") != 2 || strings.Contains(a.markdownSource(), "人员") {
		t.Fatalf("typing did not mirror the group: %q", a.markdownSource())
	}
	tr.Key(0, ui.KeyTab)
	if got := templateVisualSelection(a); got != "会议" {
		t.Fatalf("Tab selected %q instead of the next number", got)
	}
	tr.Key(ui.Shift, ui.KeyTab)
	if got := templateVisualSelection(a); got != "田小明😀" {
		t.Fatalf("Shift+Tab did not return to the edited group: %q", got)
	}
	saveWorkspaceSnapshot(t, tr, "22-workspace-template-fields")
	tr.Key(0, ui.KeyTab)
	tr.Type("周会")
	tr.Key(0, ui.KeyTab)
	if a.templateSession != nil || a.source != nil || !a.dirty {
		t.Fatal("the last Tab did not exit template editing with unsaved changes intact")
	}
	tr.Type("继续写正文")
	if got := a.markdownSource(); !strings.HasPrefix(got, "# 周会\n") || !strings.Contains(got, "确认 田小明😀\n\n继续写正文") {
		t.Fatalf("the final cursor or mirror locations were wrong: %q", got)
	}
	a.Save()
	if disk, err := os.ReadFile(a.path); err != nil || !strings.HasPrefix(string(disk), utf8BOM+"# 周会\r\n") {
		t.Fatalf("saving template edits changed the encoding or line endings: %q, %v", disk, err)
	}
}

func TestFileTreeTemplateFieldMirrorsUndoTogetherAndEscapeStopsMirroring(t *testing.T) {
	a, tr := workspaceTemplateFieldFixture(t, "{{1:值}} + {{1}}\n\n{{cursor}}\n")
	tr.Type("候选")
	if got := a.markdownSource(); got != "候选 + 候选\n\n\n" {
		t.Fatalf("same-paragraph fields were not mirrored: %q", got)
	}
	tr.Key(0, ui.KeyTab)
	tr.Key(ui.Cmd, ui.KeyZ)
	if a.templateSession != nil || a.markdownSource() != "值 + 值\n\n\n" || a.dirty {
		t.Fatalf("the mirror group did not undo together: %q", a.markdownSource())
	}
	a, tr = workspaceTemplateFieldFixture(t, "{{1:值}} + {{1}}\n")
	tr.Key(0, ui.KeyEscape)
	tr.Type("单处")
	if a.templateSession != nil || a.markdownSource() != "单处 + 值\n" {
		t.Fatalf("Esc did not end mirroring: %q", a.markdownSource())
	}
}

func TestFileTreeTemplateFieldsCanNavigateAndMirrorAuthoredMarkdown(t *testing.T) {
	a, tr := workspaceTemplateFieldFixture(t, "{{1:**强调**}} / {{1}}\n\n下一项 {{3:部分}}\n\n{{cursor}}结束\n")
	if a.source == nil || len(a.source.edit.Selections()) != 2 {
		t.Fatal("authored markup did not open both template ranges in the source editor")
	}
	tr.Type("替换😀")
	if got := a.markdownSource(); !strings.HasPrefix(got, "替换😀 / 替换😀\n") {
		t.Fatalf("the native code editor did not mirror the group: %q", got)
	}
	tr.Key(0, ui.KeyTab)
	if a.templateSession == nil || a.templateSession.numbers[a.templateSession.group] != 3 {
		t.Fatal("Tab did not advance from the first group in source view")
	}
	tr.Key(0, ui.KeyTab)
	if a.templateSession != nil || a.source.edit.Cursor() != strings.Index(a.markdownSource(), "结束") {
		t.Fatalf("the final cursor did not follow source edits: %d", a.source.edit.Cursor())
	}
}

func TestFileTreeTemplateCanFillAdjacentEmptySourceFieldsTogether(t *testing.T) {
	for _, tt := range []struct {
		name, template, filled string
		clearFirst             bool
	}{
		{"adjacent", "[链接](https://example.test/{{1:}}{{1}})\n", "[链接](https://example.test/数据数据)\n", false},
		{"separate mirror", "[链接](https://example.test/{{1:}}{{1}}) / {{1}}\n", "[链接](https://example.test/数据数据) / 数据\n", false},
		{"cleared fields", "[链接](https://example.test/{{1:x}}{{1}}) / {{1}}\n", "[链接](https://example.test/数据数据) / 数据\n", true},
	} {
		for _, input := range []string{"typing", "paste"} {
			t.Run(tt.name+"/"+input, func(t *testing.T) {
				a, tr := workspaceTemplateFieldFixture(t, tt.template)
				if tt.clearFirst {
					tr.Key(0, ui.KeyBackspace)
				}
				if a.source == nil || a.templateSession == nil || !a.templateSession.mergedSource {
					t.Fatal("the empty fields did not share a source caret")
				}
				blank := a.markdownSource()
				if input == "paste" {
					tr.SetClipboard("数据")
					tr.Key(ui.Cmd, ui.KeyV)
				} else {
					tr.Type("数据")
				}
				if got := a.markdownSource(); got != tt.filled || a.templateSession == nil {
					t.Fatalf("some source ranges did not receive the new value: %q", got)
				}
				tr.Key(ui.Cmd, ui.KeyZ)
				if a.templateSession != nil || a.dirty != tt.clearFirst || a.markdownSource() != blank {
					t.Fatalf("undo did not restore the blank fields in one step: %q", a.markdownSource())
				}
			})
		}
	}
}

func TestFileTreeTemplatePointerOutsideFieldEndsMirroring(t *testing.T) {
	a, tr := workspaceTemplateFieldFixture(t, "负责人 {{1:人员}}，确认 {{1}}。\n\n其他段落。\n")
	if a.source != nil {
		t.Fatal("the ordinary paragraph unexpectedly opened in source view")
	}
	last := a.core.Doc.Blocks[len(a.core.Doc.Blocks)-1]
	box := a.bufs[last.ID].bounds
	tr.ClickAt(box.X+8, box.Y+box.H/2)
	if a.templateSession != nil {
		t.Fatal("clicking a different paragraph left the template session open")
	}
	tr.Type("新增")
	if got := a.markdownSource(); strings.Count(got, "人员") != 2 || strings.Count(got, "新增") != 1 {
		t.Fatalf("typing outside the template fields changed their values: %q", got)
	}
}

func TestFileTreeTemplateEmptyFieldKeepsFinalCaretBeforeIt(t *testing.T) {
	a, tr := workspaceTemplateFieldFixture(t, "前缀 {{cursor}}{{1:}}结尾\n")
	tr.Type("字段")
	tr.Key(0, ui.KeyTab)
	tr.Type("光标")
	if got := a.markdownSource(); a.source != nil || a.templateSession != nil || got != "前缀 光标字段结尾\n" {
		t.Fatalf("the cursor before an empty field moved with the field's insertion: %q", got)
	}
}

func TestFileTreeTemplateNumbersUseFirstDeclaredDefaultsAndLeaveEscapes(t *testing.T) {
	out := expandWorkspaceTemplate(`{{2}} / {{2:正文}} / {{2:备选}} / \{{1:示例}}`, &templateValues{}, "\n", true)
	if out.text != "正文 / 正文 / 正文 / {{1:示例}}" || len(out.fields) != 3 {
		t.Fatalf("forward references, repeats or escaping changed: %+v", out)
	}
}
