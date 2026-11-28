package app

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"

	"github.com/neko233/markdownx/internal/diagram"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

// View draws the workspace and the active document.
func (a *App) View(c *ui.Context) {
	if len(a.tabs) == 0 {
		return
	}
	a.services = c.Services()
	a.documentTab.ensureSourceView()
	a.documentTab.claimSelection()
	if s := a.templateSession; s != nil {
		c.Root().ObserveInput(func(ev ui.InputEvent) {
			if ev.Kind == ui.InputPointerDown {
				s.outsideClick = true
			}
		})
	}
	if !a.sidebarVisible() {
		a.sidebarDrag = nil
		a.stopWorkspaceWatch()
		a.treeSearch.stop()
	}
	a.pollPDF()
	if target := a.pendingLink; target != "" {
		a.pendingLink = ""
		a.followDocumentLink(target)
	}
	if target := a.pendingNote; target.Block != 0 {
		a.pendingNote = editor.Pos{}
		a.focusAt(editor.Caret(target))
	}
	a.syncRangeInput()
	c.SetZoom(a.zoom)
	a.shortcuts(c)
	if a.treeView != nil && (!a.sidebarVisible() || a.sidebarPage == sidebarOutline || a.treeSearch.open) {
		a.treeView.closeContextMenu()
	}
	t := c.Theme()
	a.inlineColor = documentInk(t)
	a.inlineAccent = t.Accent
	a.treePointerHeld = false
	ui.Column(c).Fill().Children(func() {
		if !a.zenMode && (len(a.tabs) > 1 || a.split != nil) {
			a.tabStrip(c)
		}
		a.workspaceEditor(c)
	})
	a.encryptionDialog(c)
	a.showTemplates(c)
	a.templateDialog(c)
	a.tableDialog(c)
	a.mathDialog(c)
	a.preferences(c)
	a.showPDFDialog(c)
	a.quickOpenDialog(c)
	a.commandPaletteDialog(c)
	a.workspaceDialog(c)
	a.workspaceFilterDialog(c)
	if s := a.templateSession; s != nil && (s.outsideClick || s.source == nil && s.generation != a.core.Generation) {
		a.endWorkspaceTemplateSession()
	}
	a.tickAutoSave(c)
	a.SyncTitle()
	a.publishDocumentUpdates()
}

func (a *App) workspaceEditor(c *ui.Context) {
	ui.Row(c).Grow(1).Fill().Children(func() {
		if a.sidebarVisible() {
			a.sidebar(c)
		}
		ui.Column(c.Key("editor-column")).MinWidth(0).Grow(1).Fill().Children(func() {
			a.findBar(c)
			a.documentPanesView(c)
			a.documentStatus(c)
		})
	})
	if a.outline.owner != nil && a.outline.owner != a.documentState {
		a.outline = documentOutline{focus: a.outline.focus}
	}
	if a.sidebarVisible() && a.sidebarPage == sidebarOutline && a.source == nil &&
		a.outline.model == a.core.Doc && a.outline.generation != a.core.Generation {
		c.Invalidate()
	}
	if pane := a.pendingPane; pane != nil {
		a.pendingPane = nil
		if pane.tabOwner() == a.documentTab.tabOwner() && pane != a.documentTab {
			a.selectDocumentTab(pane, true)
			c.Invalidate()
		}
	}
}

func (a *App) documentStatus(c *ui.Context) {
	if a.zenMode && !a.dirty && a.status == "" {
		return
	}
	t := c.Theme()
	ui.Row(c.Key("document-status")).Height(workspaceFooterHeight).Padding(0, 12).Gap(10).AlignItems(ui.Center).Background(t.Surface).Children(func() {
		if !a.zenMode {
			sideLabel, glyph := "显示侧栏", "›"
			if a.sidebarVisible() {
				sideLabel, glyph = "隐藏侧栏", "‹"
			}
			side := ui.ButtonBase(c.Key("sidebar-toggle")).Size(24, 24).Center().Radius(t.Radius).
				Label(sideLabel).Tooltip(sideLabel)
			if side.Hovered() {
				side.Background(t.Selection)
			}
			side.Children(func() { ui.Text(c, glyph).FontSize(18).TextColor(t.TextMuted).Role(ui.RoleNone) })
			if side.Clicked() {
				a.toggleSidebar()
			}
			label := "源码"
			if a.source != nil {
				label = "正文"
			}
			source := ui.ButtonBase(c.Key("source-mode")).Height(24).Padding(0, 8).Radius(t.Radius).
				Label("切换 Markdown 源码").Tooltip("切换 Markdown 源码")
			if source.Hovered() {
				source.Background(t.Selection)
			}
			source.Children(func() { ui.Text(c, label).FontSize(12).TextColor(t.TextMuted) })
			if source.Clicked() {
				a.toggleSource()
			}
		}
		if a.dirty {
			ui.Text(c, "未保存").FontSize(11).TextColor(t.TextMuted).Shrink(0)
		}
		if a.status != "" {
			ui.Text(c, a.status).SingleLine().TruncateMiddle().MinWidth(0).FontSize(11).TextColor(t.TextMuted)
		}
		if !a.zenMode {
			ui.Spacer(c).Grow(1)
			ui.Text(c, a.Stats()).FontSize(11).TextColor(t.TextMuted).Shrink(0)
		}
	})
}

func (a *App) documentPaneView(c *ui.Context, pane *documentTab) {
	a.withDocumentPane(pane, func() {
		pane.ensureSourceView()
		pane.ensurePreview(a)
		ui.Row(c.Key("document-pane")).Grow(1).Fill().Children(func() {
			if a.source != nil {
				a.sourcePane(c)
				return
			}
			if a.mode != ModePreview {
				a.editPane(c)
			}
			if a.mode != ModeEdit {
				pad := float32(36)
				if pane.split != nil {
					pad = 14
				}
				ui.Scroll(c.Key("document-preview")).Grow(1).Fill().TrackScroll(&pane.previewScroll).
					Background(c.Theme().Background).Children(func() {
					ui.Column(c).Padding(24, pad).Children(func() {
						markdown.View(c, a.markdownSource(), &a.md)
					})
				})
			}
		}).ObserveInput(func(ev ui.InputEvent) {
			a.observePaneInput(pane, ev)
		})
	})
}

func (a *App) editPane(c *ui.Context) {
	scroll := ui.Scroll(c.Key("document")).Grow(1).Fill().TrackScroll(&a.editScroll)
	a.editBounds = scroll.Bounds()
	var body ui.Rect
	scroll.Children(func() {
		pad := float32(30)
		if a.split != nil {
			pad = 14
		}
		windowWidth, _ := c.Size()
		paperWidth := float32(860)
		if windowWidth >= 1800 {
			paperWidth = 1200
		} else if windowWidth >= 1400 {
			paperWidth = 1024
		}
		column := ui.Column(c).WidthPercent(100).MaxWidth(paperWidth).AlignSelf(ui.Center).
			Padding(43, pad, 100, pad).Children(func() {
			if a.typeMode {
				_, h := c.Size()
				ui.Spacer(c).Height(max(0, h/2-105))
			}
			if a.path == "" || *a.title != "" {
				e := ui.TextInputBase(c.Key("title"), a.title).FontSize(30).Bold().
					Placeholder(doc.Untitled).Label("标题").FocusRing(false)
				if a.revealTitle {
					if a.split == nil && !a.keepViewportFocus {
						e.Focus().ScrollIntoView()
					} else {
						e.Focus(ui.FocusOptions{PreventScroll: true})
					}
					a.keepViewportFocus = false
					a.revealTitle = false
				}
				if a.core.Sel.All && *a.title != "" {
					e.Background(c.Theme().Selection)
				}
				e.ObserveInput(func(ev ui.InputEvent) {
					if ev.Kind == ui.InputPointerDown && ev.Button == 0 {
						a.core.Sel = editor.Caret(editor.Pos{Block: editor.Title})
						a.drag = nil
					}
				})
				if e.Changed() {
					a.syncTitle()
				}
			}
			blocks := a.core.Doc.Blocks
			for i, b := range blocks {
				if i > 0 {
					ui.Box(c).Height(blockSpacing(blocks[i-1], b)).Shrink(0)
				} else if a.path == "" || *a.title != "" {
					ui.Box(c).Height(16).Shrink(0)
				}
				a.block(c, b, blocks, i)
			}
			if a.typeMode {
				_, h := c.Size()
				ui.Spacer(c).Height(max(0, h/2-105))
			}
		})
		body = column.Bounds()
	})
	a.dropFilesOnDocument(c, scroll, body)
}

func (a *App) showTemplates(c *ui.Context) {
	if !a.templateLibraryOpen || a.tmplOpen {
		return
	}
	ui.Modal(c, &a.templateLibraryOpen, func() {
		ui.Column(c).Width(420).Height(420).Gap(8).Padding(16).Radius(10).Background(c.Theme().Surface).Children(func() {
			ui.Text(c, "文档模板").Bold()
			a.templatePane(c)
		})
	})
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

// findBar draws the find field, match navigation and replace controls.
func (a *App) findBar(c *ui.Context) {
	if !a.finding || a.source != nil {
		return
	}
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
		e.OnTextKey(func(k ui.TextKey) bool {
			if k.Key != ui.KeyEnter {
				return false
			}
			delta := 1
			if k.Mods&ui.Shift != 0 {
				delta = -1
			}
			a.stepMatch(delta)
			return true
		})
		a.refreshMatches()
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
	if a.replacing {
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
}

// closeFind hides the find bar and drops its state.
func (a *App) closeFind() {
	a.finding = false
	a.findQuery = ""
	a.matches = nil
	a.findKey = ""
	a.matchIndex = 0
	a.replacing = false
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

func (a *App) block(c *ui.Context, b *doc.Block, siblings []*doc.Block, index int) {
	switch k := b.Kind.(type) {
	case doc.TextKind:
		a.textArea(c, b, 0)
		if _, formula := displayEquation(b); formula && a.focus == b.ID {
			if ui.Button(c.Key("edit-formula"+itoa(int(b.ID))), "编辑公式").FontSize(11).Clicked() {
				a.openEquationFor(b)
			}
		}
	case doc.HeadingKind:
		size, _ := headingMetrics(k.Level)
		a.textArea(c, b, size)
		if k.Level <= 2 {
			ui.Divider(c).Background(c.Theme().Background.Mix(c.Theme().Border, 0.42))
		}
	case doc.DefinitionKind:
		a.definitionBlock(c, b)
	case doc.TOCKind:
		a.contents(c, b)
	case doc.BulletKind:
		a.listItem(c, b, "•")
	case doc.OrderedKind:
		n := editor.OrderedNumber(siblings, index)
		if k.Seq != nil {
			n = *k.Seq
		}
		a.listItem(c, b, itoa(int(n))+".")
	case doc.TodoKind:
		a.listItem(c, b, "")
	case doc.CodeKind:
		if lang := diagram.Language(k.Lang); a.diagrams && lang != "" {
			a.editableDiagram(c, b, lang)
			return
		}
		a.codeEditor(c.Key(b.ID), b)
	case doc.QuoteKind:
		ui.Column(c).BorderColor(c.Theme().Border).BorderWidth(0, 0, 0, 4).Padding(0, 0, 0, 9).Children(func() {
			color := a.inlineColor
			a.inlineColor = c.Theme().TextMuted
			a.children(c, b)
			a.inlineColor = color
		})
	case doc.DividerKind:
		a.drawSelectedObject(c, ui.Divider(c), b.ID)
	case doc.ImageKind:
		a.editableImage(c, b)
	case doc.TableKind:
		a.table(c, b.ID, k.Table)
	case doc.FileKind:
		a.drawSelectedObject(c, ui.Text(c, "📎 "+k.File.Name).FontSize(13).TextColor(c.Theme().TextMuted), b.ID)
	case doc.InteractiveKindBlock:
		a.drawSelectedObject(c, ui.Text(c, "▶ "+firstNonEmpty(k.Interactive.Title, "交互组件")).FontSize(13).TextColor(c.Theme().TextMuted), b.ID)
	default:
		a.drawSelectedObject(c, ui.Text(c, firstNonEmpty(b.Text.Text, "（未支持的内容）")).FontSize(13).TextColor(c.Theme().TextMuted), b.ID)
	}
}

func (a *App) children(c *ui.Context, b *doc.Block) {
	if len(b.Children) == 0 {
		return
	}
	ui.Column(c).Gap(8).Children(func() {
		for i, child := range b.Children {
			a.block(c, child, b.Children, i)
		}
	})
}

func (a *App) textArea(c *ui.Context, b *doc.Block, size float32) {
	a.blockInput(c, b, size)
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
