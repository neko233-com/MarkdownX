package app

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

// View draws the window: sidebar, toolbar, editor, preview.
func (a *App) View(c *ui.Context) {
	a.shortcuts(c)
	a.SyncTitle()
	t := c.Theme()
	ui.Row(c).Fill().Children(func() {
		a.sidebar(c)
		ui.Column(c).Grow(1).Fill().Children(func() {
			a.toolbar(c)
			a.findBar(c)
			ui.Row(c).Grow(1).Fill().Children(func() {
				if a.mode != ModePreview {
					ui.Scroll(c).Grow(1).Fill().Children(func() {
						ui.Column(c).Gap(8).Padding(24, 36).Children(func() {
							ui.TextInput(c, a.title).FontSize(30).Bold().Placeholder(doc.Untitled).Label("标题")
							for i, b := range a.core.Doc.Blocks {
								a.block(c, b, i)
							}
						})
					})
				}
				if a.mode != ModeEdit {
					ui.Scroll(c).Grow(1).Fill().Background(t.Background).Children(func() {
						ui.Column(c).Padding(24, 36).Children(func() {
							markdown.View(c, a.markdownSource(), &a.md)
						})
					})
				}
			})
			ui.Row(c).Padding(6, 12).Background(t.Surface).Gap(12).Children(func() {
				ui.Text(c, a.status).FontSize(11).TextColor(t.TextMuted)
				ui.Spacer(c).Grow(1)
				ui.Text(c, a.Stats()).FontSize(11).TextColor(t.TextMuted)
			})
		})
	})
	a.encryptionDialog(c)
	a.templateDialog(c)
	a.linkDialog(c)
}

// sidebar is the left panel: 近期浏览 and 模板参考.
func (a *App) sidebar(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Width(232).FillHeight().Background(t.Surface).Padding(10, 8).Gap(8).Children(func() {
		if ui.Tabs(c, &a.sideTab, "近期浏览", "模板参考").FontSize(12).Changed() {
			a.remember()
		}
		if a.sideTab == 1 {
			a.templatePane(c)
		} else {
			a.recentPane(c)
		}
	})
}

// recentPane lists the documents opened lately, then the workspace files.
func (a *App) recentPane(c *ui.Context) {
	t := c.Theme()
	ui.Scroll(c).Fill().Children(func() {
		ui.Column(c).Gap(2).Children(func() {
			if len(a.recents) == 0 {
				ui.Text(c, "还没有打开过文档").FontSize(11).TextColor(t.TextMuted)
			}
			shown := map[string]bool{}
			for _, p := range a.recents {
				if _, err := os.Stat(p); err != nil {
					continue
				}
				shown[p] = true
				a.fileRow(c, "recent:"+p, p, filepath.Base(p), filepath.Dir(p), 0)
			}
			if a.root != "" {
				ui.Text(c, "工作区").FontSize(11).TextColor(t.TextMuted).Padding(0, 8, 4, 0)
				for _, rel := range a.files {
					full := filepath.Join(a.root, rel)
					if shown[full] {
						continue
					}
					label := strings.TrimSuffix(rel, filepath.Ext(rel))
					a.fileRow(c, "file:"+rel, full, label, "", 2)
				}
			}
		})
	})
}

// fileRow is one clickable document entry; indent separates the sections.
func (a *App) fileRow(c *ui.Context, key, full, label, tip string, indent float32) {
	t := c.Theme()
	b := ui.Button(c.Key(key), label).Label(tip).FontSize(12).Height(24).
		Justify(ui.Start).Padding(0, 8+indent)
	if a.path != "" && samePath(a.path, full) {
		b = b.Background(t.Selection)
	}
	if b.Clicked() {
		a.OpenFile(full)
	}
}

// templatePane lists the built-in and the user's templates.
func (a *App) templatePane(c *ui.Context) {
	t := c.Theme()
	ui.Row(c).Gap(4).Children(func() {
		if ui.Button(c.Key("tmpl-save"), "存为模板").Label("把当前文档存为模板").FontSize(11).Height(24).Clicked() {
			a.SaveAsTemplate()
		}
		if ui.Button(c.Key("tmpl-import"), "导入 .md").Label("把某个 .md 备份为模板").FontSize(11).Height(24).Clicked() {
			a.ImportTemplate()
		}
	})
	ui.Scroll(c).Fill().Children(func() {
		ui.Column(c).Gap(2).Children(func() {
			ui.Text(c, "内置模板").FontSize(11).TextColor(t.TextMuted).Padding(0, 0, 4, 0)
			for _, tpl := range a.templates {
				if tpl.Path == "" {
					a.templateRow(c, tpl)
				}
			}
			ui.Text(c, "我的模板").FontSize(11).TextColor(t.TextMuted).Padding(0, 8, 4, 0)
			user := 0
			for _, tpl := range a.templates {
				if tpl.Path != "" {
					a.templateRow(c, tpl)
					user++
				}
			}
			if user == 0 {
				ui.Text(c, "把某个 .md 备份为模板，就能在这里复用").FontSize(10).TextColor(t.TextMuted)
			}
		})
	})
}

// templateRow draws a template with its delete action for user templates.
func (a *App) templateRow(c *ui.Context, tpl template) {
	ui.Row(c).Gap(2).Children(func() {
		e := ui.Button(c.Key("tpl:"+tpl.Path+tpl.Name), tpl.Name).Label("插入模板").FontSize(12).Height(24).
			Justify(ui.Start).Grow(1).Padding(0, 8)
		if e.Clicked() {
			a.InsertTemplate(tpl)
		}
		if tpl.Path != "" {
			if ui.Button(c.Key("tpl-del:"+tpl.Path), "×").Label("删除模板").FontSize(12).Width(22).Height(24).Clicked() {
				a.DeleteTemplate(tpl)
			}
		}
	})
}

// encryptionDialog is the modal that asks for a password when EncryptSave
// is triggered.
func (a *App) encryptionDialog(c *ui.Context) {
	if !a.encOpen {
		return
	}
	t := c.Theme()
	ui.Modal(c, &a.encOpen, func() {
		ui.Column(c).Width(360).Padding(20, 20).Gap(12).Background(t.Surface).Radius(10).Children(func() {
			ui.Text(c, "密码保存").FontSize(18).Bold()
			ui.Text(c, "输入两次密码以加密文档；请记住密码，丢失后无法找回。").FontSize(12).TextColor(t.TextMuted)
			ui.TextInput(c, &a.encPw1).Password().Placeholder("密码").Label("密码")
			ui.TextInput(c, &a.encPw2).Password().Placeholder("确认密码").Label("确认密码")
			ui.Row(c).Gap(8).Children(func() {
				ui.Spacer(c).Grow(1)
				if ui.Button(c.Key("enc-cancel"), "取消").Clicked() {
					a.encOpen = false
					a.status = "已取消加密保存"
				}
				if ui.PrimaryButton(c.Key("enc-ok"), "加密保存").Clicked() {
					a.doEncryptSave()
				}
			})
		})
	})
}

// templateDialog names the template when saving the document as one.
func (a *App) templateDialog(c *ui.Context) {
	if !a.tmplOpen {
		return
	}
	t := c.Theme()
	ui.Modal(c, &a.tmplOpen, func() {
		ui.Column(c).Width(360).Padding(20, 20).Gap(12).Background(t.Surface).Radius(10).Children(func() {
			ui.Text(c, "存为模板").FontSize(18).Bold()
			ui.Text(c, "当前文档会保存到模板目录，之后可从「模板参考」插入。").FontSize(12).TextColor(t.TextMuted)
			ui.TextInput(c, &a.tmplName).Placeholder("模板名").Label("模板名").
				OnSubmit(func() { a.doSaveTemplate() })
			ui.Row(c).Gap(8).Children(func() {
				ui.Spacer(c).Grow(1)
				if ui.Button(c.Key("tmpl-cancel"), "取消").Clicked() {
					a.tmplOpen = false
					a.status = "已取消存为模板"
				}
				if ui.PrimaryButton(c.Key("tmpl-ok"), "保存模板").Clicked() {
					a.doSaveTemplate()
				}
			})
		})
	})
}

// linkDialog asks for the URL of a link mark.
func (a *App) linkDialog(c *ui.Context) {
	if !a.linkOpen {
		return
	}
	t := c.Theme()
	ui.Modal(c, &a.linkOpen, func() {
		ui.Column(c).Width(380).Padding(20, 20).Gap(12).Background(t.Surface).Radius(10).Children(func() {
			ui.Text(c, "插入链接").FontSize(18).Bold()
			ui.Text(c, "选中文字后按 Ctrl+K，链接会加到选区上；没有选区时作用于接下来输入的文字。").FontSize(12).TextColor(t.TextMuted)
			e := ui.TextInput(c, &a.linkURL).Placeholder("https://").Label("链接地址")
			if a.linkFocus {
				e.Focus()
				a.linkFocus = false
			}
			e.OnSubmit(func() { a.doInsertLink() })
			ui.Row(c).Gap(8).Children(func() {
				ui.Spacer(c).Grow(1)
				if ui.Button(c.Key("link-cancel"), "取消").Clicked() {
					a.linkOpen = false
					a.status = "已取消插入链接"
				}
				if ui.PrimaryButton(c.Key("link-ok"), "插入").Clicked() {
					a.doInsertLink()
				}
			})
		})
	})
}

// findBar draws the find field, match navigation and replace controls.
func (a *App) findBar(c *ui.Context) {
	if !a.finding {
		return
	}
	a.refreshMatches()
	t := c.Theme()
	ui.Row(c).Padding(6, 12).Gap(6).Background(t.Surface).Children(func() {
		e := ui.TextInput(c.Key("find-q"), &a.findQuery).Placeholder("查找").Label("查找").FontSize(12).Width(220)
		if a.findFocus {
			e.Focus()
			a.findFocus = false
		}
		if e.Changed() {
			a.matchIndex = 0
		}
		count := "0 / 0"
		if len(a.matches) > 0 {
			count = itoa(a.matchIndex+1) + " / " + itoa(len(a.matches))
		}
		ui.Text(c, count).FontSize(11).TextColor(t.TextMuted)
		if ui.Button(c.Key("find-prev"), "↑").Label("上一个").FontSize(12).Height(24).Padding(0, 8).Clicked() {
			a.stepMatch(-1)
		}
		if ui.Button(c.Key("find-next"), "↓").Label("下一个").FontSize(12).Height(24).Padding(0, 8).Clicked() {
			a.stepMatch(1)
		}
		ui.Spacer(c).Grow(1)
		if ui.Button(c.Key("find-close"), "关闭").Label("关闭查找").FontSize(11).Height(24).Padding(0, 8).Clicked() {
			a.closeFind()
		}
	})
	ui.Row(c).Padding(4, 12).Gap(8).Background(t.Surface).Children(func() {
		ui.TextInput(c, &a.findReplace).Placeholder("替换为").Label("替换为").FontSize(12).Width(200)
		if ui.Button(c.Key("repl-one"), "替换").Label("替换当前").FontSize(12).Height(24).Padding(0, 8).Clicked() {
			a.replaceOne(a.findReplace)
		}
		if ui.Button(c.Key("repl-all"), "全部替换").Label("全部替换").FontSize(12).Height(24).Padding(0, 8).Clicked() {
			a.replaceAll(a.findReplace)
		}
	})
}

// closeFind hides the find bar and drops its state.
func (a *App) closeFind() {
	a.finding = false
	a.findQuery = ""
	a.matches = nil
	a.findKey = ""
	a.matchIndex = 0
}

// stepMatch moves to the next or previous hit, wrapping around.
func (a *App) stepMatch(delta int) {
	n := len(a.matches)
	if n == 0 {
		return
	}
	a.matchIndex = ((a.matchIndex+delta)%n + n) % n
	a.jumpToMatch()
}

// toolbar is the formatting strip above the editor.
func (a *App) toolbar(c *ui.Context) {
	t := c.Theme()
	ui.Row(c).Padding(6, 8).Gap(1).Background(t.Surface).Children(func() {
		btn := func(key, label, tip string, fn func()) {
			if ui.Button(c.Key(key), label).Label(tip).FontSize(11).Height(26).Padding(0, 6).Clicked() {
				fn()
			}
		}
		btn("h1", "H1", "标题 1", func() { a.heading(1) })
		btn("h2", "H2", "标题 2", func() { a.heading(2) })
		btn("h3", "H3", "标题 3", func() { a.heading(3) })
		btn("p", "¶", "正文", func() { a.heading(0) })
		ui.Divider(c)
		btn("b", "B", "加粗", a.bold)
		btn("i", "I", "斜体", a.italic)
		btn("u", "U", "下划线", a.underline)
		btn("s", "S", "删除线", a.strike)
		btn("code", "</>", "行内代码", a.inlineCode)
		ui.Divider(c)
		btn("ul", "•", "项目符号", a.bullet)
		btn("ol", "1.", "编号列表", a.ordered)
		btn("todo", "☑", "待办", a.todo)
		btn("quote", "❝", "引用", a.quote)
		btn("pre", "{ }", "代码块", a.code)
		btn("hr", "—", "分割线", a.divider)
		btn("table", "▦", "插入表格", a.insertTable)
		btn("img", "🖼", "插入图片", a.insertImage)
		ui.Divider(c)
		btn("undo", "↶", "撤销", a.undo)
		btn("redo", "↷", "重做", a.redo)
		ui.Divider(c)
		modeLabel := "分栏"
		switch a.mode {
		case ModeEdit:
			modeLabel = "编辑"
		case ModePreview:
			modeLabel = "预览"
		}
		btn("mode", modeLabel, "切换视图", func() { a.setMode((a.mode + 1) % 3) })
		btn("save", "保存", "保存", a.Save)
	})
}

func (a *App) block(c *ui.Context, b *doc.Block, index int) {
	switch k := b.Kind.(type) {
	case doc.TextKind:
		a.textArea(c, b, 0)
	case doc.HeadingKind:
		size := float32(30 - int(k.Level)*2)
		if size < 16 {
			size = 16
		}
		a.textArea(c, b, size)
	case doc.BulletKind:
		ui.Row(c).Gap(8).Children(func() {
			ui.Text(c, "•").FontSize(16)
			ui.Column(c).Grow(1).Children(func() {
				a.textArea(c, b, 15)
				a.children(c, b)
			})
		})
	case doc.OrderedKind:
		n := editor.OrderedNumber(a.core.Doc.Blocks, index)
		if k.Seq != nil {
			n = *k.Seq
		}
		ui.Row(c).Gap(8).Children(func() {
			ui.Textf(c, "%d.", n).FontSize(15)
			ui.Column(c).Grow(1).Children(func() {
				a.textArea(c, b, 15)
				a.children(c, b)
			})
		})
	case doc.TodoKind:
		ui.Row(c).Gap(8).Children(func() {
			box := "☐"
			if k.Done {
				box = "☑"
			}
			if ui.Button(c.Key(b.ID), box).Label("已完成").Width(30).Clicked() {
				a.core.ToggleTodoDone(b.ID)
			}
			ui.Column(c).Grow(1).Children(func() {
				a.textArea(c, b, 15)
				a.children(c, b)
			})
		})
	case doc.CodeKind:
		ui.Column(c).Padding(12).Radius(6).Background(c.Theme().Surface).Children(func() {
			a.textArea(c, b, 13)
		})
	case doc.QuoteKind:
		ui.Row(c).Gap(10).Children(func() {
			ui.Column(c).Width(3).Background(c.Theme().Border)
			ui.Column(c).Grow(1).Children(func() { a.children(c, b) })
		})
	case doc.DividerKind:
		ui.Divider(c)
	case doc.ImageKind:
		ui.Text(c, "🖼 "+deref(k.Caption, k.Src)).FontSize(13).TextColor(c.Theme().TextMuted)
	case doc.TableKind:
		a.table(c, k.Table)
	case doc.FileKind:
		ui.Text(c, "📎 "+k.File.Name).FontSize(13).TextColor(c.Theme().TextMuted)
	case doc.InteractiveKindBlock:
		ui.Text(c, "▶ "+firstNonEmpty(k.Interactive.Title, "交互组件")).FontSize(13).TextColor(c.Theme().TextMuted)
	default:
		ui.Text(c, firstNonEmpty(b.Text.Text, "（未支持的内容）")).FontSize(13).TextColor(c.Theme().TextMuted)
	}
}

func (a *App) children(c *ui.Context, b *doc.Block) {
	if len(b.Children) == 0 {
		return
	}
	ui.Column(c).Padding(0, 0, 0, 14).Gap(6).Children(func() {
		for i, child := range b.Children {
			a.block(c, child, i)
		}
	})
}

func (a *App) table(c *ui.Context, t *doc.Table) {
	if t == nil {
		return
	}
	ui.Column(c).Gap(1).Children(func() {
		for r := range t.Rows {
			ui.Row(c).Gap(1).Children(func() {
				for cidx := range t.Rows[r] {
					cell := &t.Rows[r][cidx]
					if cell.IsCovered() {
						continue
					}
					inHeader := r < t.HeaderRows
					ui.Column(c).Grow(1).Padding(4, 6).Children(func() {
						for _, cb := range cell.Blocks {
							if !cb.Kind.HasText() {
								continue
							}
							e := a.blockInput(c, cb, 13)
							if inHeader {
								e.Bold()
							}
						}
					})
				}
			})
		}
	})
}

func (a *App) textArea(c *ui.Context, b *doc.Block, size float32) {
	a.blockInput(c, b, size)
}

// blockInput renders one editable line and returns its element, so a caller
// can style it.
func (a *App) blockInput(c *ui.Context, b *doc.Block, size float32) ui.Element {
	e := ui.TextArea(c, a.buf(b.ID)).Lines(1, 40).Label("正文")
	if size > 0 {
		e.FontSize(size)
	}
	if e.Changed() {
		a.focus = b.ID
		a.sync(b)
		// Typora-style: a Markdown marker typed at the start turns the
		// paragraph into the block it names. The block it becomes owns the
		// caret from here on, so this input stops updating the focus.
		start, end := e.TextSelection()
		text := *a.buf(b.ID)
		at := doc.RuneToByte(text, start)
		if start == end {
			a.sel[b.ID] = [2]int{at, at}
			if a.autoFormat(b, at) {
				return e
			}
		}
	}
	if e.Focused() {
		a.focus = b.ID
		start, end := e.TextSelection()
		text := *a.buf(b.ID)
		a.sel[b.ID] = [2]int{doc.RuneToByte(text, start), doc.RuneToByte(text, end)}
		if p, ok := a.pending[b.ID]; ok {
			e.SetTextSelection(p[0], p[1])
			delete(a.pending, b.ID)
		}
	} else if a.focusReq == b.ID {
		// A find match asks for the focus once.
		e.Focus()
		if p, ok := a.pending[b.ID]; ok {
			e.SetTextSelection(p[0], p[1])
			delete(a.pending, b.ID)
		}
		a.focusReq = 0
	}
	e.OnTextKey(func(k ui.TextKey) bool {
		a.focus = b.ID
		return a.onKey(k, b)
	})
	return e
}

func (a *App) onKey(k ui.TextKey, b *doc.Block) bool {
	switch k.Key {
	case ui.KeyEnter:
		a.sync(b)
		a.core.Sel = editor.Caret(editor.Pos{Block: b.ID, Offset: a.caretOf(b)})
		a.core.Enter()
		// Typing continues in the block Enter created, not the one it split.
		target := a.core.Sel.Head.Block
		a.ResetBuffers()
		a.focus = target
		a.focusReq = target
		a.restoreSelection(target, 0, 0)
		return true
	case ui.KeyTab:
		a.sync(b)
		a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
		if k.Mods&ui.Shift != 0 {
			a.core.Outdent()
		} else {
			a.core.Indent()
		}
		a.ResetBuffers()
		return true
	case ui.KeyBackspace:
		if a.caretOf(b) > 0 {
			return false // the input deletes the character
		}
		a.sync(b)
		a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
		a.core.Backspace()
		a.ResetBuffers()
		return true
	}
	return false
}

// caretOf is the focused block's caret in byte offsets.
func (a *App) caretOf(b *doc.Block) int {
	if b.ID == a.focus {
		if s, ok := a.sel[b.ID]; ok {
			at := s[1]
			return clampOffset(b.Text.Text, at)
		}
	}
	return b.Text.Len()
}

func (a *App) shortcuts(c *ui.Context) {
	switch {
	case c.Shortcut(ui.Cmd, ui.KeyS):
		a.Save()
	case c.Shortcut(ui.Cmd, ui.KeyO):
		a.ChooseFile()
	case c.Shortcut(ui.Cmd, ui.KeyN):
		a.NewDocument()
	case c.Shortcut(ui.Cmd, ui.KeyB):
		a.bold()
	case c.Shortcut(ui.Cmd, ui.KeyI):
		a.italic()
	case c.Shortcut(ui.Cmd, ui.KeyU):
		a.underline()
	case c.Shortcut(ui.Cmd, ui.KeyD):
		a.duplicate()
	case c.Shortcut(ui.Cmd, ui.KeyE):
		a.inlineCode()
	case c.Shortcut(ui.Cmd, ui.KeyK):
		a.openLink()
	case c.Shortcut(ui.Cmd, ui.KeyT):
		a.insertTable()
	case c.Shortcut(ui.Cmd, ui.KeyZ):
		a.undo()
	case c.Shortcut(ui.Cmd|ui.Shift, ui.KeyZ):
		a.redo()
	case c.Shortcut(ui.Cmd, ui.KeyY):
		a.redo()
	case c.Shortcut(ui.Cmd|ui.Shift, ui.KeyK):
		a.code()
	case c.Shortcut(ui.Cmd|ui.Shift, ui.KeyQ):
		a.quote()
	case c.Shortcut(ui.Cmd|ui.Shift, ui.KeyC):
		a.todo()
	case c.Shortcut(ui.Cmd|ui.Shift, ui.KeyBracketLeft):
		a.bullet()
	case c.Shortcut(ui.Cmd|ui.Shift, ui.KeyBracketRight):
		a.ordered()
	case c.Shortcut(ui.Cmd|ui.Shift, ui.KeyEnter):
		a.insertBelow()
	case c.Shortcut(ui.Cmd, ui.KeyF):
		a.finding = true
		a.findFocus = true
		a.findKey = ""
	case c.Shortcut(ui.Cmd, ui.KeyH):
		a.finding = true
		a.findFocus = true
		a.findKey = ""
	}
	for level := 0; level <= 6; level++ {
		if c.Shortcut(ui.Cmd, digitKey(level)) {
			a.heading(level)
			return
		}
	}
}

func digitKey(n int) ui.Key {
	switch n {
	case 1:
		return ui.Key1
	case 2:
		return ui.Key2
	case 3:
		return ui.Key3
	case 4:
		return ui.Key4
	case 5:
		return ui.Key5
	case 6:
		return ui.Key6
	default:
		return ui.Key0
	}
}
