package app

import (
	"fmt"
	"runtime"
	"strings"

	"github.com/egoist/mygo/ui"
	sourceeditor "github.com/egoist/mygo/ui/editor"

	"github.com/neko233/markdownx/internal/editor"
)

type commandScope uint8

const (
	windowCommand commandScope = iota
	documentCommand
	bodyCommand
	tableCommand
)

type command struct {
	menu        string
	label       string
	accelerator string
	key         ui.Key
	mods        ui.Modifiers
	scope       commandScope
	run         func(*App)
}

var commands []command

func init() { commands = commandsFor(runtime.GOOS) }

// Both the native menu and the editor use these bindings.
func commandsFor(platform string) []command {
	commandKey := ui.Ctrl
	if platform == "darwin" {
		commandKey = ui.Super
	}
	platformKey := func(windows, mac string) string {
		if platform == "darwin" {
			return mac
		}
		return windows
	}
	var list []command
	add := func(menu, label, accelerator string, scope commandScope, run func(*App)) {
		key, mods := resolveCommandKey(accelerator, commandKey)
		list = append(list, command{menu, label, accelerator, key, mods, scope, run})
	}
	const file, edit, paragraph, format, view = "文件", "编辑", "段落", "格式", "视图"
	add(file, "新建", "CmdOrCtrl+N", windowCommand, (*App).NewDocument)
	add(file, "新建标签页", platformKey("", "Cmd+T"), windowCommand, (*App).newTab)
	add(file, "新建窗口", "CmdOrCtrl+Shift+N", windowCommand, (*App).newWindow)
	add(file, "打开…", "CmdOrCtrl+O", windowCommand, (*App).ChooseFile)
	add(file, "快速打开…", platformKey("Ctrl+P", "Cmd+Shift+O"), windowCommand, (*App).openQuickOpen)
	add(file, "最近文档…", "", windowCommand, (*App).openRecent)
	add(file, "打开文件夹…", platformKey("Ctrl+Shift+O", ""), windowCommand, (*App).ChooseFolder)
	add(file, "重新打开已关闭的文件", "CmdOrCtrl+Shift+T", windowCommand, (*App).reopenClosed)
	add(file, "保存", "CmdOrCtrl+S", windowCommand, (*App).Save)
	add(file, "制作文档副本", platformKey("", "Cmd+Shift+S"), windowCommand, (*App).duplicateDocument)
	add(file, "另存为 Markdown…", platformKey("Ctrl+Shift+S", "Cmd+Option+Shift+S"), windowCommand, (*App).SaveAs)
	add(file, "全部保存", platformKey("", "Cmd+Option+S"), windowCommand, (*App).saveAllDocuments)
	add(file, "密码保存…", "", windowCommand, (*App).EncryptSave)
	add(file, "沿用上次的导出格式…", "Ctrl+Shift+E", windowCommand, (*App).exportPrevious)
	add(file, "导出 HTML…", "", windowCommand, (*App).ExportHTML)
	add(file, "导出 PDF…", "", windowCommand, (*App).ExportPDF)
	add(file, "打印预览…", platformKey("Alt+Shift+P", "Cmd+P"), windowCommand, (*App).printPreview)
	add(file, "导出书籍 (.mdxb)…", "", windowCommand, (*App).ExportBook)
	add(file, "存为模板…", "", windowCommand, (*App).SaveAsTemplate)
	add(file, "模板…", "", windowCommand, func(a *App) { a.templateLibraryOpen = true })
	add(file, "偏好设置…", "CmdOrCtrl+,", windowCommand, func(a *App) { a.prefsOpen = true })
	add(file, "关闭标签页", "CmdOrCtrl+W", windowCommand, (*App).closeActiveTab)
	add(file, "关闭窗口", "", windowCommand, func(a *App) {
		if a.win != nil {
			a.win.Close()
		}
	})

	add(edit, "撤销", "CmdOrCtrl+Z", documentCommand, func(a *App) {
		a.endWorkspaceTemplateSession()
		if a.source != nil {
			a.source.edit.Run(sourceeditor.Undo)
		} else {
			a.undo()
		}
	})
	add(edit, "重做", "CmdOrCtrl+Shift+Z", documentCommand, (*App).redoCommand)
	add("", "重做", "CmdOrCtrl+Y", documentCommand, (*App).redoCommand)
	add(edit, "复制为 Markdown", "CmdOrCtrl+Shift+C", documentCommand, (*App).copyMarkdown)
	add(edit, "粘贴纯文本", "CmdOrCtrl+Shift+V", documentCommand, (*App).pastePlain)
	add(edit, "选择词语", "CmdOrCtrl+D", documentCommand, func(a *App) { a.selectWord(false) })
	add(edit, "删除词语", "Ctrl+Shift+D", documentCommand, func(a *App) { a.selectWord(true) })
	add(edit, "选择段落或块", "CmdOrCtrl+Alt+P", documentCommand, func(a *App) { a.editTextRange(paragraphRange, false) })
	add(edit, "选择行或句子", "CmdOrCtrl+L", documentCommand, func(a *App) { a.editTextRange(sentenceRange, false) })
	add(edit, "选择行内格式范围", "CmdOrCtrl+E", bodyCommand, func(a *App) { a.editTextRange(styleRange, false) })
	add(edit, "删除段落或块", "CmdOrCtrl+Alt+Shift+P", documentCommand, func(a *App) { a.editTextRange(paragraphRange, true) })
	add(edit, "删除行或句子", "CmdOrCtrl+Alt+Shift+L", documentCommand, func(a *App) { a.editTextRange(sentenceRange, true) })
	add(edit, "删除行内格式范围", "CmdOrCtrl+Alt+Shift+E", bodyCommand, func(a *App) { a.editTextRange(styleRange, true) })
	add(edit, "上移段落或表格行", "Alt+Up", bodyCommand, func(a *App) { a.moveParagraph(-1) })
	add(edit, "下移段落或表格行", "Alt+Down", bodyCommand, func(a *App) { a.moveParagraph(1) })
	add(edit, "跳到文档开头", platformKey("Ctrl+Home", "Cmd+Up"), documentCommand, func(a *App) { a.jumpDocumentEdge(false) })
	add(edit, "跳到当前位置", "CmdOrCtrl+J", documentCommand, (*App).jumpSelection)
	add(edit, "跳到文档末尾", platformKey("Ctrl+End", "Cmd+Down"), documentCommand, func(a *App) { a.jumpDocumentEdge(true) })
	add(edit, "查找…", "CmdOrCtrl+F", windowCommand, func(a *App) { a.openFind(false) })
	add(edit, "查找下一个", "F3", documentCommand, func(a *App) { a.findRelative(1) })
	add(edit, "查找上一个", "Shift+F3", documentCommand, func(a *App) { a.findRelative(-1) })
	add(edit, "替换…", "Ctrl+H", windowCommand, func(a *App) { a.openFind(true) })
	add(edit, "复制当前块", "", bodyCommand, (*App).duplicate)

	for level := 1; level <= 6; level++ {
		add(paragraph, "标题 "+itoa(level), "Ctrl+"+itoa(level), bodyCommand, func(a *App) { a.heading(level) })
	}
	add(paragraph, "正文", "Ctrl+0", bodyCommand, func(a *App) { a.heading(0) })
	add(paragraph, "增加标题级别", "Ctrl+=", bodyCommand, func(a *App) { a.changeHeading(1) })
	add(paragraph, "降低标题级别", "Ctrl+-", bodyCommand, func(a *App) { a.changeHeading(-1) })
	add(paragraph, "表格…", "Ctrl+T", bodyCommand, (*App).openTableSize)
	add(paragraph, "代码块", "Ctrl+Shift+K", bodyCommand, (*App).code)
	add(paragraph, "公式块…", "Ctrl+Shift+M", bodyCommand, (*App).openEquation)
	add(paragraph, "引用", "Ctrl+Shift+Q", bodyCommand, (*App).quote)
	add(paragraph, "编号列表", "Ctrl+Shift+[", bodyCommand, (*App).ordered)
	add(paragraph, "项目符号", "Ctrl+Shift+]", bodyCommand, (*App).bullet)
	add(paragraph, "待办列表", "Ctrl+Shift+X", bodyCommand, (*App).todo)
	add(paragraph, "切换待办状态", platformKey("", "Ctrl+X"), bodyCommand, (*App).toggleTaskStatus)
	add(paragraph, "增加缩进", "CmdOrCtrl+]", bodyCommand, func(a *App) { a.changeIndent(false) })
	add(paragraph, "减少缩进", "CmdOrCtrl+[", bodyCommand, func(a *App) { a.changeIndent(true) })
	add(paragraph, "分割线", "", bodyCommand, (*App).divider)
	add(paragraph, "目录", "", bodyCommand, (*App).insertContents)
	add(paragraph, "下方插入段落", "", bodyCommand, (*App).insertBelow)
	add(paragraph, "选择表格行", "", tableCommand, func(a *App) { a.selectTableCell(true) })
	add(paragraph, "选择单元格", "", tableCommand, func(a *App) { a.selectTableCell(false) })
	add(paragraph, "左移表格列", "Alt+Left", tableCommand, func(a *App) { a.moveTableColumn(-1) })
	add(paragraph, "右移表格列", "Alt+Right", tableCommand, func(a *App) { a.moveTableColumn(1) })
	add(paragraph, "在下方插入表格行", "Ctrl+Enter", tableCommand, func(a *App) {
		a.changeTable((*editor.Core).InsertTableRow)
	})
	add(paragraph, "删除表格行", "Ctrl+Shift+Backspace", tableCommand, func(a *App) {
		a.changeTable((*editor.Core).DeleteTableRow)
	})

	add(format, "加粗", "CmdOrCtrl+B", bodyCommand, (*App).bold)
	add(format, "斜体", "CmdOrCtrl+I", bodyCommand, (*App).italic)
	add(format, "下划线", "CmdOrCtrl+U", bodyCommand, (*App).underline)
	add(format, "行内代码", "CmdOrCtrl+Shift+`", bodyCommand, (*App).inlineCode)
	add(format, "删除线", "Alt+Shift+5", bodyCommand, (*App).strike)
	add(format, "链接…", "Ctrl+K", bodyCommand, (*App).openLink)
	add(format, "图片…", "CmdOrCtrl+Shift+I", bodyCommand, (*App).insertImage)
	add(format, "清除行内格式", "CmdOrCtrl+\\", bodyCommand, (*App).clearFormatting)

	add(view, "仅编辑", "CmdOrCtrl+Alt+1", windowCommand, func(a *App) { a.setMode(ModeEdit) })
	add(view, "分栏", "CmdOrCtrl+Alt+2", windowCommand, func(a *App) { a.setMode(ModeSplit) })
	add(view, "向右分栏", "CmdOrCtrl+Alt+\\", windowCommand, (*App).splitRight)
	add(view, "聚焦左侧分栏", platformKey("", "Cmd+Ctrl+Left"), windowCommand, func(a *App) { a.stepSplit(-1) })
	add(view, "聚焦右侧分栏", platformKey("", "Cmd+Ctrl+Right"), windowCommand, func(a *App) { a.stepSplit(1) })
	add(view, "关闭分栏", "", windowCommand, func(a *App) { a.closeSplitPane(a.documentTab) })
	add(view, "仅预览", "CmdOrCtrl+Alt+3", windowCommand, func(a *App) { a.setMode(ModePreview) })
	add(view, "切换侧栏", "CmdOrCtrl+Shift+L", windowCommand, (*App).toggleSidebar)
	add(view, "搜索工作区…", "CmdOrCtrl+Shift+F", windowCommand, (*App).showWorkspaceSearch)
	add(view, "大纲", platformKey("Ctrl+Shift+1", "Cmd+Ctrl+1"), windowCommand, (*App).showDocumentOutline)
	add(view, "文件树", platformKey("Ctrl+Shift+3", "Cmd+Ctrl+3"), windowCommand, (*App).showWorkspaceTree)
	add(view, "切换 Markdown 源码", "CmdOrCtrl+/", windowCommand, (*App).toggleSource)
	add(view, "专注模式", "F8", windowCommand, (*App).toggleFocusMode)
	add(view, "打字机模式", "F9", windowCommand, (*App).toggleTypewriter)
	add(view, "沉浸模式", "CmdOrCtrl+Shift+Enter", windowCommand, (*App).toggleZen)
	add(view, "命令面板…", "CmdOrCtrl+Shift+P", windowCommand, (*App).openCommandPalette)
	add(view, "全屏", "F11", windowCommand, (*App).fullscreen)
	add(view, "实际大小", "CmdOrCtrl+Shift+0", windowCommand, func(a *App) { a.zoom = 1 })
	add(view, "放大", "CmdOrCtrl+Shift+=", windowCommand, func(a *App) { a.zoomBy(0.125) })
	add(view, "缩小", "CmdOrCtrl+Shift+-", windowCommand, func(a *App) { a.zoomBy(-0.125) })
	add(view, "下一个标签页", "Ctrl+Tab", windowCommand, func(a *App) { a.stepTab(1) })
	add(view, "上一个标签页", "Ctrl+Shift+Tab", windowCommand, func(a *App) { a.stepTab(-1) })
	add(view, "切换到下一个窗口", platformKey("", "Cmd+`"), windowCommand, (*App).nextWindow)
	return list
}

var commandKeys = map[string]ui.Key{
	"F3": ui.KeyF3, "F8": ui.KeyF8, "F9": ui.KeyF9, "F11": ui.KeyF11,
	"Enter": ui.KeyEnter, "Backspace": ui.KeyBackspace, "Tab": ui.KeyTab,
	"Home": ui.KeyHome, "End": ui.KeyEnd, "Up": ui.KeyUp, "Down": ui.KeyDown, "Left": ui.KeyLeft, "Right": ui.KeyRight,
	"[": ui.KeyBracketLeft, "]": ui.KeyBracketRight, "\\": ui.KeyBackslash,
	",": ui.KeyComma, "/": ui.KeySlash, "`": ui.KeyBackquote, "-": ui.KeyMinus, "=": ui.KeyEqual,
}

func resolveCommandKey(accelerator string, primary ui.Modifiers) (ui.Key, ui.Modifiers) {
	if accelerator == "" {
		return ui.KeyUnknown, 0
	}
	parts := strings.Split(accelerator, "+")
	var mods ui.Modifiers
	for _, part := range parts[:len(parts)-1] {
		switch part {
		case "Ctrl":
			mods |= ui.Ctrl
		case "Cmd":
			mods |= ui.Super
		case "CmdOrCtrl":
			mods |= primary
		case "Alt", "Option":
			mods |= ui.Alt
		case "Shift":
			mods |= ui.Shift
		default:
			panic(fmt.Sprintf("unknown modifier in %q", accelerator))
		}
	}
	last := parts[len(parts)-1]
	if len(last) == 1 {
		switch k := last[0]; {
		case k >= 'A' && k <= 'Z':
			return ui.KeyA + ui.Key(k-'A'), mods
		case k >= '0' && k <= '9':
			return ui.Key0 + ui.Key(k-'0'), mods
		}
	}
	if key, ok := commandKeys[last]; ok {
		return key, mods
	}
	panic(fmt.Sprintf("unknown key in %q", accelerator))
}

func (a *App) commandDialogOpen() bool {
	return a.quickOpen || a.palette.open || a.templateLibraryOpen || a.encOpen || a.tmplOpen ||
		a.prefsOpen || a.pdfDialog.open || a.tableSize.open || a.equation.open || a.treeEdit.open || a.treeFilterEdit.open
}

func (a *App) accepts(cmd command) bool {
	return !a.commandDialogOpen() && a.commandAvailable(cmd)
}

func (a *App) commandAvailable(cmd command) bool {
	switch cmd.scope {
	case documentCommand:
		return a.mode != ModePreview || a.source != nil
	case bodyCommand:
		return a.focused() != nil
	case tableCommand:
		_, ok := a.core.TableCellAt(a.focus)
		return a.source == nil && a.mode != ModePreview && ok
	}
	return true
}

func (a *App) shortcuts(c *ui.Context) {
	for _, cmd := range commands {
		if cmd.key != ui.KeyUnknown && a.accepts(cmd) && c.Shortcut(cmd.mods, cmd.key) {
			cmd.run(a)
		}
	}
	if !a.commandDialogOpen() && (a.treeSearch.open && a.sidebarVisible() || a.finding || a.zenMode) && c.Shortcut(0, ui.KeyEscape) {
		if a.finding {
			a.closeFind()
		} else if a.treeSearch.open && a.sidebarVisible() {
			a.showWorkspaceTree()
		} else {
			a.toggleZen()
		}
	}
}

func (a *App) editorShortcut(mods ui.Modifiers, key ui.Key) bool {
	if key == ui.KeyUnknown {
		return false
	}
	for _, cmd := range commands {
		if cmd.key == key && cmd.mods == mods && a.accepts(cmd) {
			cmd.run(a)
			return true
		}
	}
	return false
}

func (a *App) redoCommand() {
	a.endWorkspaceTemplateSession()
	if a.source != nil {
		a.source.edit.Run(sourceeditor.Redo)
	} else {
		a.redo()
	}
}

func (a *App) findRelative(delta int) {
	if a.source != nil {
		cmd := sourceeditor.FindNext
		if delta < 0 {
			cmd = sourceeditor.FindPrevious
		}
		a.source.edit.Run(cmd)
		return
	}
	if a.findQuery == "" {
		a.openFind(false)
		return
	}
	a.refreshMatches()
	a.stepMatch(delta)
}
