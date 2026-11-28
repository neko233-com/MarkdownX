package app

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func TestDesktopCommandKeys(t *testing.T) {
	type binding struct {
		label   string
		windows string
		mac     string
	}
	want := []binding{
		{"制作文档副本", "", "Cmd+Shift+S"},
		{"另存为 Markdown…", "Ctrl+Shift+S", "Cmd+Option+Shift+S"},
		{"全部保存", "", "Cmd+Option+S"},
		{"沿用上次的导出格式…", "Ctrl+Shift+E", "Ctrl+Shift+E"},
		{"打印预览…", "Alt+Shift+P", "Cmd+P"},
		{"复制为 Markdown", "CmdOrCtrl+Shift+C", "CmdOrCtrl+Shift+C"},
		{"粘贴纯文本", "CmdOrCtrl+Shift+V", "CmdOrCtrl+Shift+V"},
		{"选择词语", "CmdOrCtrl+D", "CmdOrCtrl+D"},
		{"快速打开…", "Ctrl+P", "Cmd+Shift+O"},
		{"表格…", "Ctrl+T", "Ctrl+T"},
		{"在下方插入表格行", "Ctrl+Enter", "Ctrl+Enter"},
		{"删除表格行", "Ctrl+Shift+Backspace", "Ctrl+Shift+Backspace"},
		{"左移表格列", "Alt+Left", "Alt+Left"},
		{"右移表格列", "Alt+Right", "Alt+Right"},
		{"编号列表", "Ctrl+Shift+[", "Ctrl+Shift+["},
		{"项目符号", "Ctrl+Shift+]", "Ctrl+Shift+]"},
		{"增加缩进", "CmdOrCtrl+]", "CmdOrCtrl+]"},
		{"减少缩进", "CmdOrCtrl+[", "CmdOrCtrl+["},
		{"待办列表", "Ctrl+Shift+X", "Ctrl+Shift+X"},
		{"切换待办状态", "", "Ctrl+X"},
		{"代码块", "Ctrl+Shift+K", "Ctrl+Shift+K"},
		{"公式块…", "Ctrl+Shift+M", "Ctrl+Shift+M"},
		{"引用", "Ctrl+Shift+Q", "Ctrl+Shift+Q"},
		{"标题 1", "Ctrl+1", "Ctrl+1"},
		{"标题 6", "Ctrl+6", "Ctrl+6"},
		{"正文", "Ctrl+0", "Ctrl+0"},
		{"增加标题级别", "Ctrl+=", "Ctrl+="},
		{"降低标题级别", "Ctrl+-", "Ctrl+-"},
		{"链接…", "Ctrl+K", "Ctrl+K"},
		{"图片…", "CmdOrCtrl+Shift+I", "CmdOrCtrl+Shift+I"},
		{"删除线", "Alt+Shift+5", "Alt+Shift+5"},
		{"全屏", "F11", "F11"},
		{"缩小", "CmdOrCtrl+Shift+-", "CmdOrCtrl+Shift+-"},
		{"大纲", "Ctrl+Shift+1", "Cmd+Ctrl+1"},
		{"文件树", "Ctrl+Shift+3", "Cmd+Ctrl+3"},
		{"搜索工作区…", "CmdOrCtrl+Shift+F", "CmdOrCtrl+Shift+F"},
		{"选择行内格式范围", "CmdOrCtrl+E", "CmdOrCtrl+E"},
		{"选择行或句子", "CmdOrCtrl+L", "CmdOrCtrl+L"},
		{"选择段落或块", "CmdOrCtrl+Alt+P", "CmdOrCtrl+Alt+P"},
		{"命令面板…", "CmdOrCtrl+Shift+P", "CmdOrCtrl+Shift+P"},
		{"沉浸模式", "CmdOrCtrl+Shift+Enter", "CmdOrCtrl+Shift+Enter"},
		{"替换…", "Ctrl+H", "Ctrl+H"},
		{"下一个标签页", "Ctrl+Tab", "Ctrl+Tab"},
		{"上一个标签页", "Ctrl+Shift+Tab", "Ctrl+Shift+Tab"},
		{"切换到下一个窗口", "", "Cmd+`"},
		{"新建标签页", "", "Cmd+T"},
		{"关闭标签页", "CmdOrCtrl+W", "CmdOrCtrl+W"},
		{"向右分栏", "CmdOrCtrl+Alt+\\", "CmdOrCtrl+Alt+\\"},
		{"聚焦左侧分栏", "", "Cmd+Ctrl+Left"},
		{"聚焦右侧分栏", "", "Cmd+Ctrl+Right"},
	}
	for _, platform := range []string{"windows", "darwin"} {
		t.Run(platform, func(t *testing.T) {
			found := make(map[string]string)
			used := make(map[[2]uint16]string)
			for _, cmd := range commandsFor(platform) {
				if cmd.accelerator == "" {
					continue
				}
				found[cmd.label] = cmd.accelerator
				b := [2]uint16{uint16(cmd.key), uint16(cmd.mods)}
				if prev, ok := used[b]; ok {
					t.Fatalf("conflicting key for %q and %q", prev, cmd.label)
				}
				used[b] = cmd.label
			}
			for _, item := range want {
				spec := item.windows
				if platform == "darwin" {
					spec = item.mac
				}
				if got := found[item.label]; got != spec {
					t.Errorf("%s uses %q, want %q", item.label, got, spec)
				}
			}
		})
	}
}

func TestClipboardFormatsSeparatedByShortcut(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("**alpha** beta")
	b := a.core.Doc.Blocks[0]
	a.focusAt(editor.Selection{Anchor: editor.Pos{Block: b.ID}, Head: editor.Pos{Block: b.ID, Offset: 5}})
	tr := ui.NewTester(a.View, 900, 680)
	tr.Frame()
	tr.Key(ui.Cmd|ui.Shift, ui.KeyC)
	if got := tr.Clipboard(); got != "**alpha**" {
		t.Fatalf("copy-as-Markdown wrote %q", got)
	}
	plain := "# 标题\n- 项目\n1. 开始\n$literal$"
	tr.SetClipboard(plain)
	tr.Key(ui.Cmd|ui.Shift, ui.KeyV)
	tr.Frame()
	if got := b.Text.Text; got != plain+" beta" {
		t.Fatalf("plain paste = %q", got)
	}
	blocks := doc.MarkdownToBlocks(a.markdownSource(), doc.Off{File: true})
	if len(blocks) != 1 || blocks[0].Text.Text != plain+" beta" {
		t.Fatalf("plain paste changed Markdown structure: %q", a.markdownSource())
	}
}

func TestListIndentAndOutdentKeepBlockIdentity(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("- 第一项\n- 第二项\n")
	first, second := a.core.Doc.Blocks[0], a.core.Doc.Blocks[1]
	a.focusAt(editor.Caret(editor.Pos{Block: second.ID, Offset: len(second.Text.Text)}))
	tr := ui.NewTester(a.View, 900, 680)
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyBracketRight)
	tr.Frame()
	if len(a.core.Doc.Blocks) != 1 || len(first.Children) != 1 || first.Children[0] != second {
		t.Fatal("indent removed the current block instead of nesting it")
	}
	tr.Key(ui.Cmd, ui.KeyBracketLeft)
	tr.Frame()
	if len(a.core.Doc.Blocks) != 2 || len(first.Children) != 0 || a.core.Doc.Blocks[1] != second {
		t.Fatal("outdent did not restore the same list block")
	}
}

func TestWordCommandsSelectAndDeleteUnicode(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("甲 café 乙")
	b := a.core.Doc.Blocks[0]
	a.focusAt(editor.Caret(editor.Pos{Block: b.ID, Offset: len("甲 ca")}))
	tr := ui.NewTester(a.View, 900, 680)
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyD)
	tr.Frame()
	if selected := a.core.SelectedText(); selected != "café" {
		t.Fatalf("selected %q", selected)
	}
	tr.Key(ui.Ctrl|ui.Shift, ui.KeyD)
	tr.Frame()
	if len(a.core.Doc.Blocks) != 1 || b.Text.Text != "甲  乙" {
		t.Fatalf("delete word left %q", b.Text.Text)
	}
}

func TestTableShortcutsSelectCopyInsertReorderAndDelete(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("| Key | Value |\n| --- | --- |\n| A | **first** |\n| B | second |\n")
	table := a.core.Doc.Blocks[0].Kind.(doc.TableKind).Table
	b := table.Rows[2][0].Blocks[0]
	a.focusAt(editor.Caret(editor.Pos{Block: b.ID}))
	tr := ui.NewTester(a.View, 950, 680)
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyL)
	tr.Frame()
	tr.Key(ui.Cmd|ui.Shift, ui.KeyC)
	if got := tr.Clipboard(); got != "| B | second |" {
		t.Fatalf("copy selected row = %q", got)
	}
	tr.Key(ui.Alt, ui.KeyUp)
	tr.Frame()
	if got := table.Rows[1][0].Blocks[0]; got.ID != b.ID {
		t.Fatalf("Alt+Up did not move B above A: %q", got.Text.Text)
	}
	tr.Key(ui.Ctrl, ui.KeyEnter)
	tr.Frame()
	if len(table.Rows) != 4 || a.focus != table.Rows[2][0].Blocks[0].ID {
		t.Fatal("Ctrl/Cmd+Enter did not insert and focus the matching cell")
	}
	tr.Type("新行")
	tr.Frame()
	tr.Key(ui.Ctrl|ui.Shift, ui.KeyBackspace)
	tr.Frame()
	if len(table.Rows) != 3 || strings.Contains(a.markdownSource(), "新行") {
		t.Fatalf("delete row left %q", a.markdownSource())
	}
	a.undo()
	if !strings.Contains(a.markdownSource(), "新行") {
		t.Fatal("undo did not restore deleted row")
	}
}

func TestWindowReopenHistorySkipsMissingFiles(t *testing.T) {
	dir := isolateConfig(t)
	path := filepath.Join(dir, "note.md")
	if err := os.WriteFile(path, []byte("note"), 0o600); err != nil {
		t.Fatal(err)
	}
	d := &desktopWindows{}
	a := New(path)
	d.order, d.active = []*App{a}, a
	d.remove(a)
	d.rememberClosed(filepath.Join(dir, "missing.md"))
	if got := d.nextClosed(); got != path || d.active != nil || len(d.order) != 0 {
		t.Fatalf("reopen returned %q with live state %+v", got, d)
	}
}
