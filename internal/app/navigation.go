package app

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	sourceeditor "github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/egoist/mygo/ui/markdown/parser"

	"github.com/neko233/markdownx/internal/convert"
	"github.com/neko233/markdownx/internal/doc"
)

func (a *App) sidebarVisible() bool { return !a.hideSide && !a.zenMode }

func (a *App) toggleSidebar() {
	if a.zenMode {
		a.zenMode = false
		a.hideSide = true
	}
	a.hideSide = !a.hideSide
	if !a.hideSide && a.sidebarPage == sidebarFiles {
		a.revealWorkspaceFile()
	}
	a.remember()
}

func (a *App) toggleZen() {
	a.zenMode = !a.zenMode
	a.focusPaneCaret()
}

func (a *App) toggleFocusMode() {
	a.focusMode = !a.focusMode
	a.remember()
}

func (a *App) toggleTypewriter() {
	a.typeMode = !a.typeMode
	a.reveal = a.focus
	a.remember()
}

func (a *App) jumpHeading(d *doc.Document, id doc.BlockID) {
	if a.source == nil && a.mode != ModePreview {
		a.focusHeading(id)
	}
	if a.source == nil && a.mode == ModeEdit {
		return
	}
	ordinal := slices.IndexFunc(d.Headings(markdown.Slug), func(h doc.Heading) bool { return h.Block == id })
	if ordinal < 0 {
		return
	}
	if a.source != nil {
		parsed := parser.Parse(a.source.text())
		if i := parsedHeading(parsed, ordinal); i > 0 {
			at := parsed.Block(i).Start
			a.source.edit.SetSelections(sourceeditor.Selection{Anchor: at, Head: at})
			a.source.focus = true
		}
		return
	}
	a.md.Anchor = "markdownx-outline"
	a.md.AnchorVersion++
	a.md.AnchorBlock = func(d *parser.Document, name string) int {
		if name == "markdownx-outline" {
			return parsedHeading(d, ordinal)
		}
		return -1
	}
}

func parsedHeading(d *parser.Document, ordinal int) int {
	for i := 1; i < d.Len(); i++ {
		b := d.Block(i)
		if b.Kind == parser.FootnoteDefinition {
			i = b.Next - 1
			continue
		}
		if b.Kind == parser.Heading {
			if ordinal == 0 {
				return i
			}
			ordinal--
		}
	}
	return -1
}

func (a *App) openQuickOpen() {
	if a.root != "" {
		a.refreshFiles()
	}
	a.quickOpen, a.quickFocus = true, true
	a.quickRecent, a.quickActions = false, false
	a.quickQuery, a.quickIndex = "", 0
	a.quickCategory = quickAll
}

func (a *App) openRecent() {
	a.quickOpen, a.quickFocus, a.quickRecent = true, true, true
	a.quickActions = false
	a.quickQuery, a.quickIndex = "", 0
	a.quickCategory = quickRecents
}

type quickCategory uint8

const (
	quickAll quickCategory = iota
	quickOpened
	quickRecents
	quickFolders
	quickFiles
	quickHeadings
)

var quickCategoryLabels = [...]string{"全部", "已打开", "最近", "文件夹", "文件", "跳到标题"}

type quickHit struct {
	path, name string
	tab        *documentTab
	heading    doc.BlockID
}

func (a *App) quickResults() []quickHit {
	query := strings.Fields(strings.ToLower(a.quickQuery))
	var direct, fuzzy []quickHit
	seen := make(map[quickHit]bool)
	add := func(hit quickHit, name string) {
		key := quickHit{tab: hit.tab, heading: hit.heading}
		if hit.path != "" {
			key.path, key.tab = workspacePathKey(filepath.Clean(hit.path)), nil
		}
		if seen[key] || len(direct) >= 100 {
			return
		}
		name = strings.ToLower(name)
		match, isFuzzy := matchQuickText(name, query)
		if !match {
			return
		}
		seen[key] = true
		if !isFuzzy {
			direct = append(direct, hit)
		} else if len(fuzzy) < 100 {
			fuzzy = append(fuzzy, hit)
		}
	}
	category := a.quickCategory
	if category == quickAll || category == quickOpened {
		for _, tab := range a.tabs {
			for _, pane := range tab.panes() {
				if pane.path != "" || pane.dirty || category == quickOpened {
					name := pane.DisplayName()
					add(quickHit{path: pane.path, name: name, tab: pane}, name+" "+pane.path)
				}
			}
		}
	}
	if category == quickAll || category == quickRecents {
		for _, p := range a.recents {
			if _, err := os.Stat(p); err == nil {
				add(quickHit{path: p, name: filepath.Base(p)}, p)
			}
		}
	}
	if category == quickAll || category == quickFolders {
		s := loadSettings()
		for _, folders := range [][]string{s.PinnedFolders, s.RecentFolders} {
			for _, path := range folders {
				if info, err := os.Stat(path); err == nil && info.IsDir() {
					add(quickHit{path: path, name: filepath.Base(path)}, path)
				}
			}
		}
	}
	if category == quickHeadings || category == quickAll && len(query) > 0 {
		for _, h := range a.outlineDocument().Headings(markdown.Slug) {
			name := firstNonEmpty(h.Text, "空标题")
			add(quickHit{name: name, heading: h.Block}, name)
		}
	}
	if category == quickAll || category == quickFiles {
		for _, rel := range a.files {
			if convert.Detect(rel) != convert.FormatUnknown {
				add(quickHit{path: filepath.Join(a.root, rel), name: rel}, rel)
			}
		}
	}
	return append(direct, fuzzy[:min(len(fuzzy), 100-len(direct))]...)
}

func (a *App) outlineDocument() *doc.Document {
	if a.source != nil {
		return a.source.document()
	}
	return a.core.Doc
}

func matchQuickText(name string, query []string) (match, fuzzy bool) {
	for _, word := range query {
		if strings.Contains(name, word) {
			continue
		}
		fuzzy = true
		rest := name
		for _, r := range word {
			var ok bool
			_, rest, ok = strings.Cut(rest, string(r))
			if !ok {
				return false, false
			}
		}
	}
	return true, fuzzy
}

func (a *App) quickOpenDialog(c *ui.Context) {
	if !a.quickOpen {
		return
	}
	ui.Modal(c, &a.quickOpen, func() {
		ui.Column(c).Width(620).Padding(16).Gap(8).Background(c.Theme().Surface).Radius(10).Children(func() {
			label := "快速打开"
			if a.quickRecent {
				label = "最近文档"
			}
			e := ui.TextInput(c.Key("quick-open"), &a.quickQuery).Label(label).Placeholder("搜索文件名、路径或标题")
			if a.quickFocus {
				e.Focus()
				a.quickFocus = false
			}
			if e.Changed() {
				a.quickIndex = 0
				a.quickActions = false
			}
			if !a.quickRecent {
				ui.Row(c).Gap(4).Children(func() {
					for i, name := range quickCategoryLabels {
						category := quickCategory(i)
						button := ui.Button(c.Key(category), name).FontSize(12).AccessSelected(category == a.quickCategory)
						if a.quickCategory == category {
							button.Background(c.Theme().Selection)
						}
						if button.Clicked() {
							a.changeQuickCategory(category)
						}
					}
				})
			}
			hits := a.quickResults()
			a.quickIndex = max(0, min(a.quickIndex, len(hits)-1))
			if len(hits) > 0 && c.Shortcut(ui.Cmd, ui.KeyK) {
				a.quickActions = !a.quickActions
			}
			e.OnTextKey(func(k ui.TextKey) bool {
				switch k.Key {
				case ui.KeyDown:
					a.quickIndex = max(0, min(a.quickIndex+1, len(hits)-1))
				case ui.KeyUp:
					a.quickIndex = max(0, a.quickIndex-1)
				case ui.KeyTab:
					if a.quickRecent || a.quickActions {
						return false
					}
					step := 1
					if k.Mods&ui.Shift != 0 {
						step = -1
					}
					n := len(quickCategoryLabels)
					a.changeQuickCategory(quickCategory((int(a.quickCategory) + n + step) % n))
				case ui.KeyEnter:
					if len(hits) > 0 {
						a.chooseQuickHit(hits[a.quickIndex])
					}
				default:
					return false
				}
				return true
			})
			ui.Row(c).Height(242).Gap(10).Children(func() {
				ui.Scroll(c).Grow(1).MinWidth(0).FillHeight().Children(func() {
					ui.Column(c).Gap(2).Children(func() {
						if len(hits) == 0 {
							ui.Text(c, "没有匹配结果").TextColor(c.Theme().TextMuted)
						}
						for i, hit := range hits {
							row := ui.ButtonBase(c.Key(hit)).Height(27).FillWidth().Padding(0, 7).
								Justify(ui.Start).Label(firstNonEmpty(hit.path, hit.name)).AccessSelected(i == a.quickIndex)
							row.Children(func() { ui.Text(c, hit.name).SingleLine().TruncateMiddle().MinWidth(0) })
							row.ContextMenu(func(menu *ui.Menu) { a.quickHitActions(menu, hit) })
							if i == a.quickIndex {
								row.Background(c.Theme().Selection).ScrollIntoView()
							}
							if row.Clicked() {
								a.chooseQuickHit(hit)
							}
						}
					})
				})
				if len(hits) > 0 {
					a.quickPreview(c, hits[a.quickIndex])
				}
			})
			if len(hits) > 0 {
				hit := hits[a.quickIndex]
				ui.PopoverMenuButton(c.Key("quick-location-actions"), "操作", func(menu *ui.Menu) {
					a.quickHitActions(menu, hit)
				}, ui.PopoverMenuButtonOptions{Open: &a.quickActions}).AlignSelf(ui.End).Tooltip("Ctrl / Cmd + K")
			}
		})
	})
}

func (a *App) changeQuickCategory(category quickCategory) {
	a.quickIndex, a.quickActions = 0, false
	a.quickCategory, a.quickFocus = category, true
}

func (a *App) chooseQuickHit(hit quickHit) {
	a.quickOpen = false
	switch {
	case hit.heading != 0:
		a.jumpHeading(a.outlineDocument(), hit.heading)
	case hit.tab != nil:
		a.selectDocumentTab(hit.tab, true)
	default:
		a.openQuickResult(hit.path)
	}
}

func (a *App) quickHitActions(menu *ui.Menu, hit quickHit) {
	if hit.heading != 0 || hit.path == "" {
		label := "跳到标题"
		if hit.heading == 0 {
			label = "切换到标签页"
		}
		if menu.Item(label).Chosen() {
			a.chooseQuickHit(hit)
		}
		return
	}
	a.quickLocationActions(menu, hit.path)
}

func (a *App) quickPreview(c *ui.Context, hit quickHit) {
	t := c.Theme()
	ui.Scroll(c.Key("quick-preview")).Width(226).FillHeight().Padding(10).
		Background(t.Background).Border(1, t.Border).Radius(t.Radius).Children(func() {
		ui.Column(c).Gap(6).MinWidth(0).Children(func() {
			ui.Text(c, hit.name).Bold().FontSize(12).MaxLines(2)
			var preview workspacePreview
			if hit.tab != nil {
				a.withDocumentPane(hit.tab, func() { preview.excerpt = workspaceExcerpt(a.markdownSource()) })
			} else if node := a.tree.at(hit.path); node != nil && !node.isDir() {
				preview = a.workspacePreviewContent(node, workspaceRasterPreview(hit.path))
			}
			if preview.picture != nil {
				w, h := ui.ImageSize(preview.picture)
				scale := min(float32(1), 198/w, 194/h)
				ui.Image(c, preview.picture).Size(w*scale, h*scale).AlignSelf(ui.Center)
			} else if preview.excerpt != "" {
				markdown.View(c, preview.excerpt, &markdown.Options{
					LoadImage: func(string) (ui.ImageSource, error) { return nil, errors.New("预览只读取文档摘录") },
				})
			} else if hit.path != "" {
				ui.Text(c, hit.path).FontSize(11).TextColor(t.TextMuted)
			}
		})
	})
}

func (a *App) quickLocationActions(menu *ui.Menu, path string) {
	if node := a.tree.at(path); node != nil {
		if menu.Item("在目录树中显示").Chosen() {
			a.quickOpen = false
			a.showWorkspaceTree()
			a.focusTreeEntry(node.path)
		}
		menu.Separator()
	}
	if convert.Detect(path) != convert.FormatUnknown {
		for i, label := range []string{"在当前标签页打开", "在新标签页打开", "在新窗口打开"} {
			if menu.Item(label).Chosen() {
				a.quickOpen = false
				a.openWorkspaceFile(path, workspaceOpenMode(i))
			}
		}
		if menu.Item("在分栏中打开").Chosen() {
			a.quickOpen = false
			a.openDocumentInSplit(path, -1)
		}
		menu.Separator()
	}
	a.workspacePathMenu(menu, path)
}

func (a *App) openQuickResult(path string) {
	if info, err := os.Stat(path); err == nil && info.IsDir() {
		a.openWorkspaceFolder(path)
		return
	}
	a.openWorkspaceFile(path, loadSettings().TreeOpenMode)
}
