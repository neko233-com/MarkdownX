package app

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	sourceeditor "github.com/egoist/mygo/ui/editor"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func TestLiveInlineMarkdownAndHardBreak(t *testing.T) {
	isolateConfig(t)
	a := New("")
	id := a.core.Doc.Blocks[0].ID
	a.focusAt(editor.Caret(editor.Pos{Block: id}))
	tr := ui.NewTester(a.View, 920, 680)
	tr.Frame()
	for _, c := range "**粗体** *斜体* ~~完成~~" {
		tr.Type(string(c))
		tr.Frame()
	}
	b := a.core.Doc.Blocks[0]
	spans := b.Text.Spans()
	if b.Text.Text != "粗体 斜体 完成" || len(spans) != 5 ||
		!spans[0].Marks.Bold || !spans[2].Marks.Italic || !spans[4].Marks.Strike {
		t.Fatalf("live text and marks = %#v", b.Text)
	}
	tr.Key(ui.Shift, ui.KeyEnter)
	tr.Type("下一行")
	tr.Frame()
	if got := a.markdownSource(); !strings.Contains(got, "~~完成~~  \n下一行") || len(a.core.Doc.Blocks) != 1 {
		t.Fatalf("hard break changed block structure: %q", got)
	}
}

func TestSourceModeReusesExactTextAndCommitsEdits(t *testing.T) {
	dir := isolateConfig(t)
	path := filepath.Join(dir, "source.md")
	raw := utf8BOM + "Title\r\n=====\r\n\r\nA _word_.\r\n\r\n## Next\r\n"
	if err := os.WriteFile(path, []byte(raw), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	a.mode = ModeEdit
	head := a.core.Doc.Blocks[2].ID
	a.focusAt(editor.Caret(editor.Pos{Block: head, Offset: 4}))
	tr := ui.NewTester(a.View, 900, 640)
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeySlash)
	tr.Frame()
	if a.source == nil {
		t.Fatal("shortcut did not open Markdown source")
	}
	expect := strings.ReplaceAll(strings.TrimPrefix(raw, utf8BOM), "\r\n", "\n")
	if got := a.source.text(); got != expect {
		t.Fatalf("exact source = %q, want %q", got, expect)
	}
	if at := a.source.edit.Selection().Head; at != len(expect)-1 {
		t.Fatalf("source caret = %d, want %d", at, len(expect)-1)
	}
	tr.Type(" 2")
	tr.Frame()
	if !a.dirty {
		t.Fatal("source edit did not mark the file dirty")
	}
	tr.Key(ui.Cmd, ui.KeySlash)
	tr.Frame()
	b := a.core.Doc.Blocks[2]
	if a.source != nil || b.Text.Text != "Next 2" || b.ID != a.focus {
		t.Fatalf("source edit did not restore the edited heading: %#v", b.Text)
	}
	a.Save()
	if got, err := os.ReadFile(path); err != nil || string(got) != strings.Replace(raw, "## Next", "## Next 2", 1) || a.dirty {
		t.Fatalf("save readback = %q, dirty=%v, err=%v, status=%s", got, a.dirty, err, a.status)
	}
	a.undo()
	if got := a.markdownSource(); got != expect {
		t.Fatalf("undo lost original Markdown spelling: %q", got)
	}
}

func TestSourceOutlineJumpAndSaveWithoutToggling(t *testing.T) {
	dir := isolateConfig(t)
	path := filepath.Join(dir, "outline.md")
	raw := "# Repeat\n\nFirst\n\n## Repeat\n\nSecond\n"
	if err := os.WriteFile(path, []byte(raw), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	a.toggleSource()
	d := a.source.document()
	a.jumpHeading(d, d.Blocks[2].ID)
	if at := a.source.edit.Selection().Head; at != strings.Index(raw, "## Repeat") {
		t.Fatalf("jump used wrong repeated title: %d", at)
	}
	a.source.edit.Buffer.Set(raw + "\n\\*literal\\*\n")
	a.source.edit.SetSelections(sourceeditor.Selection{})
	a.Save()
	if got, err := os.ReadFile(path); err != nil || string(got) != raw+"\n\\*literal\\*\n" {
		t.Fatalf("source save = %q, %v", got, err)
	}
	a.leaveSource()
	if a.dirty {
		t.Fatal("switching away from a saved source made it dirty")
	}
}

func TestVisualSavePreservesUntouchedBlocksAndExternalChanges(t *testing.T) {
	dir := isolateConfig(t)
	path := filepath.Join(dir, "visual.md")
	const original = "---\ntitle: 写作\n---\n\n![alt](<assets/preview file.svg>)\n\nEdit this.\n\nUse [short][id].\n\n[id]: ./other.md\n\n"
	if err := os.WriteFile(path, []byte(original), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	b := a.core.Doc.Blocks[2]
	a.core.ReplaceInBlock(b.ID, 0, 4, "Change")
	a.ResetBuffers()
	a.Save()
	want := strings.Replace(original, "Edit this.", "Change this.", 1)
	if got, err := os.ReadFile(path); err != nil || string(got) != want {
		t.Fatalf("local edit rewrote untouched syntax: %q; %v", got, err)
	}
	if err := os.WriteFile(path, []byte("External edit\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a.core.ReplaceInBlock(b.ID, 0, len("Change"), "Unsaved")
	a.ResetBuffers()
	a.Save()
	if got, err := os.ReadFile(path); err != nil || string(got) != "External edit\n" || !a.dirty {
		t.Fatalf("external edit overwritten: %q, dirty=%v, err=%v", got, a.dirty, err)
	}
}

func TestPipeTableAndCellKeyboardNavigation(t *testing.T) {
	isolateConfig(t)
	a := New("")
	a.focusAt(editor.Caret(editor.Pos{Block: a.core.Doc.Blocks[0].ID}))
	tr := ui.NewTester(a.View, 920, 680)
	tr.Frame()
	tr.Type("| 任务 | 状态 |")
	tr.Key(0, ui.KeyEnter)
	tr.Frame()
	k, ok := a.core.Doc.Blocks[0].Kind.(doc.TableKind)
	if !ok || len(k.Table.Rows) != 2 {
		t.Fatalf("typed header did not form a table: %T", a.core.Doc.Blocks[0].Kind)
	}
	tr.Type("校稿")
	tr.Key(0, ui.KeyTab)
	tr.Type("完成")
	tr.Frame()
	if k.Table.Rows[1][0].Blocks[0].Text.Text != "校稿" || k.Table.Rows[1][1].Blocks[0].Text.Text != "完成" {
		t.Fatal("Tab did not navigate horizontally")
	}
	tr.Key(0, ui.KeyTab)
	tr.Frame()
	if len(k.Table.Rows) != 3 || a.focus != k.Table.Rows[2][0].Blocks[0].ID {
		t.Fatal("Tab in the last cell did not start a new row")
	}
	a.undo()
	if got := len(a.core.Doc.Blocks[0].Kind.(doc.TableKind).Table.Rows); got != 2 {
		t.Fatalf("undo table row insertion: got %d rows", got)
	}
}

func TestReplaceAllUsesOriginalUnicodeOffsets(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("K想 K想 猫")
	a.findQuery = "k想"
	a.replaceAll("书")
	if got := a.core.Doc.PlainText(); got != "书 书 猫" || !a.dirty {
		t.Fatalf("replace = %q, dirty=%v", got, a.dirty)
	}
	a.undo()
	if got := a.core.Doc.PlainText(); got != "K想 K想 猫" {
		t.Fatalf("undo = %q", got)
	}
}

func TestAutoSaveWritesAfterTypingStops(t *testing.T) {
	dir := isolateConfig(t)
	path := filepath.Join(dir, "autosave.md")
	if err := os.WriteFile(path, []byte("Alpha\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	a.autoSaveOn = true
	id := a.core.Doc.Blocks[0].ID
	a.focusAt(editor.Caret(editor.Pos{Block: id, Offset: len("Alpha")}))
	tr := ui.NewTester(a.View, 920, 680)
	tr.Frame()
	tr.Type(" β")
	tr.Frame()
	if got, err := os.ReadFile(path); err != nil || string(got) != "Alpha\n" {
		t.Fatalf("auto-saved before the delay: %q, %v", got, err)
	}
	tr.Advance(2 * time.Second)
	tr.Frame()
	if got, err := os.ReadFile(path); err != nil || string(got) != "Alpha β\n" || a.dirty {
		t.Fatalf("auto-save = %q, dirty=%v, %v", got, a.dirty, err)
	}
}

func TestNarrowWindowKeepsNavigationAndSourceSwitchVisible(t *testing.T) {
	isolateConfig(t)
	a := New("")
	tr := ui.NewTester(a.View, 760, 520)
	tr.Frame()
	if chooser, ok := tr.Find("打开文件夹…"); !ok || chooser.X+chooser.W > 264 {
		t.Fatalf("the tree's open-folder button is clipped: %#v", chooser)
	}
	if source, ok := tr.Find("切换 Markdown 源码"); !ok || source.X+source.W > 760 || source.Y+source.H > 520 {
		t.Fatalf("source switch is clipped: %#v", source)
	}
}

func TestTaskCheckboxUpdatesSaveStateAndUndo(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("- [ ] 写完")
	tr := ui.NewTester(a.View, 920, 680)
	tr.Frame()
	if err := tr.Click("已完成"); err != nil {
		t.Fatal(err)
	}
	if got := a.markdownSource(); !a.dirty || !strings.HasPrefix(got, "- [x] 写完") {
		t.Fatalf("checkbox toggle = %q, dirty=%v", got, a.dirty)
	}
	a.undo()
	if got := a.markdownSource(); !strings.HasPrefix(got, "- [ ] 写完") {
		t.Fatalf("checkbox undo = %q", got)
	}
}
