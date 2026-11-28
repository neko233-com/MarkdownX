package app

import (
	"context"
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	sourceeditor "github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/markdown/parser"

	"github.com/neko233/markdownx/internal/convert"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type workspaceSearchKey struct {
	root     string
	revision uint64
	query    workspaceQuery
	liveFile string
	model    *editor.Core
	source   *sourceView
	version  uint64
}

type workspaceSearchState struct {
	open, focus, focusResults bool
	query                     workspaceQuery
	key                       workspaceSearchKey
	due                       time.Time
	cancel                    context.CancelFunc
	ready                     <-chan workspaceSearchResult
	result                    workspaceSearchResult
	tree                      *workspaceSearchNode
	openPaths                 map[string]*bool
}

func (s *workspaceSearchState) stop() {
	if s.cancel != nil {
		s.cancel()
	}
	s.cancel, s.ready = nil, nil
	s.due = time.Time{}
	s.key = workspaceSearchKey{}
	s.result = workspaceSearchResult{}
	s.tree = nil
}

func (a *App) showWorkspaceSearch() {
	a.setSidebarPage(sidebarFiles)
	a.treeSearch.open, a.treeSearch.focus = true, true
	if a.tree == nil {
		a.treeFocus = true
	}
	a.remember()
}

func (a *App) refreshWorkspaceSearch(c *ui.Context) {
	s := &a.treeSearch
	key := workspaceSearchKey{root: a.root, revision: a.treeRevision, query: s.query}
	if active := a.activeWorkspacePath(); active != "" && !s.query.names && a.dirty {
		key.liveFile, _ = workspaceRel(a.root, active)
		key.model, key.source = a.core, a.source
		key.version = a.core.Generation
		if a.source != nil {
			key.version = a.source.edit.Buffer.Version()
		}
	}
	if key != s.key {
		if s.key.root != key.root || s.key.query != key.query {
			s.openPaths = make(map[string]*bool)
		}
		s.stop()
		s.key = key
		if strings.TrimSpace(key.query.text) != "" {
			s.due = c.Now().Add(160 * time.Millisecond)
		}
	}
	select {
	case result := <-s.ready:
		s.cancel()
		s.cancel, s.ready = nil, nil
		s.result = result
		s.tree = workspaceResultTree(result.hits)
	default:
	}
	if s.due.IsZero() {
		return
	}
	if wait := s.due.Sub(c.Now()); wait > 0 {
		c.After(wait)
		return
	}
	s.due = time.Time{}
	ctx, cancel := context.WithCancel(context.Background())
	ready := make(chan workspaceSearchResult, 1)
	s.ready, s.cancel = ready, cancel
	files, services := a.files, c.Services()
	live := workspaceLiveText{relative: key.liveFile}
	if live.relative != "" {
		live.content = a.markdownSource()
	}
	go func() {
		result := scanWorkspace(ctx, key.root, files, key.query, live)
		if ctx.Err() == nil {
			ready <- result
			services.Invalidate()
		}
	}()
}

func (a *App) workspaceSearchPane(c *ui.Context) {
	s := &a.treeSearch
	ui.Column(c.Key("workspace-search")).Grow(1).Fill().Gap(6).Children(func() {
		if ui.Button(c.Key("end-search"), "返回文件树").FontSize(12).AlignSelf(ui.Start).Clicked() {
			a.showWorkspaceTree()
			return
		}
		input := ui.TextInput(c.Key("workspace-query"), &s.query.text).
			Label("工作区搜索").Placeholder("搜索 " + filepath.Base(a.root)).FontSize(13).FillWidth()
		if s.focus {
			input.Focus()
			s.focus = false
		}
		input.OnTextKey(func(key ui.TextKey) bool {
			switch key.Key {
			case ui.KeyEnter:
				if len(s.result.hits) > 0 {
					a.openWorkspaceHit(s.result.hits[0])
				}
				return true
			case ui.KeyDown:
				s.focusResults = true
				a.services.Invalidate()
				return true
			}
			return false
		})
		ui.Row(c).Gap(10).Children(func() {
			ui.Radio(c.Key("find-content"), &s.query.names, false, "正文").FontSize(12)
			ui.Radio(c.Key("find-name"), &s.query.names, true, "文件名").FontSize(12)
		})
		ui.Row(c).Gap(7).Children(func() {
			ui.Checkbox(c.Key("match-case"), &s.query.matchCase, "区分大小写").FontSize(11)
			ui.Checkbox(c.Key("match-word"), &s.query.wholeWord, "整词").FontSize(11)
			ui.Checkbox(c.Key("match-regex"), &s.query.expression, "正则").FontSize(11)
		})
		a.refreshWorkspaceSearch(c)
		if status := s.message(); status != "" {
			ui.Text(c, status).FontSize(11).TextColor(c.Theme().TextMuted).Wrap()
		}
		ui.Scroll(c.Key("workspace-search-results")).Grow(1).Fill().Children(func() {
			if s.tree != nil {
				ui.Tree(c, func() {
					for _, node := range s.tree.children {
						a.workspaceResultItem(c, node)
					}
				}).Label("工作区搜索结果")
			}
		})
	})
}

func (s *workspaceSearchState) message() string {
	if s.result.err != nil {
		return s.result.err.Error()
	}
	if s.ready != nil || !s.due.IsZero() {
		return "正在搜索…"
	}
	if strings.TrimSpace(s.query.text) == "" {
		return ""
	}
	unit := "行"
	if s.query.names {
		unit = "个文件"
	}
	text := itoa(len(s.result.hits)) + " " + unit + "匹配"
	if s.result.capped {
		text = "显示前 " + text
	}
	if s.result.skipped > 0 {
		text += "，跳过 " + itoa(s.result.skipped) + " 个过大、不可读或非 UTF-8 的文档"
	}
	return text
}

type workspaceSearchNode struct {
	relative string
	children []*workspaceSearchNode
	hits     []workspaceHit
}

func workspaceResultTree(hits []workspaceHit) *workspaceSearchNode {
	if len(hits) == 0 {
		return nil
	}
	root := &workspaceSearchNode{relative: "."}
	byPath := map[string]*workspaceSearchNode{".": root}
	var add func(string) *workspaceSearchNode
	add = func(path string) *workspaceSearchNode {
		if node := byPath[path]; node != nil {
			return node
		}
		parent := add(filepath.Dir(path))
		node := &workspaceSearchNode{relative: path}
		byPath[path] = node
		parent.children = append(parent.children, node)
		return node
	}
	for _, hit := range hits {
		node := add(hit.relative)
		node.hits = append(node.hits, hit)
	}
	for _, node := range byPath {
		slices.SortStableFunc(node.children, func(left, right *workspaceSearchNode) int {
			if len(left.hits) == 0 && len(right.hits) != 0 {
				return -1
			}
			if len(right.hits) == 0 && len(left.hits) != 0 {
				return 1
			}
			return naturalPathOrder(left.relative, right.relative)
		})
	}
	return root
}

func (a *App) workspaceResultItem(c *ui.Context, node *workspaceSearchNode) {
	path := filepath.Join(a.root, node.relative)
	name := filepath.Base(node.relative)
	if len(node.hits) > 0 && node.hits[0].line == 0 {
		row := ui.TreeItem(c.Key(node.relative), name, nil, nil).FontSize(12).Description(path)
		a.focusWorkspaceResult(row)
		if row.Clicked() {
			a.openWorkspaceHit(node.hits[0])
		}
		return
	}
	s := &a.treeSearch
	open := s.openPaths[node.relative]
	if open == nil {
		expanded := true
		open = &expanded
		s.openPaths[node.relative] = open
	}
	row := ui.TreeItem(c.Key(node.relative), name, open, func() {
		for _, child := range node.children {
			a.workspaceResultItem(c, child)
		}
		for _, hit := range node.hits {
			label := itoa(hit.line) + "   " + hit.excerpt
			item := ui.TreeItem(c.Key(hit.line), label, nil, nil).FontSize(12).Description(name + " 第 " + itoa(hit.line) + " 行")
			a.focusWorkspaceResult(item)
			if item.Clicked() {
				a.openWorkspaceHit(hit)
			}
		}
	}).FontSize(12).Description(path)
	a.focusWorkspaceResult(row)
	if row.Clicked() {
		*open = !*open
		a.services.Invalidate()
	}
}

func (a *App) focusWorkspaceResult(row ui.Element) {
	if a.treeSearch.focusResults {
		row.Focus().ScrollIntoView()
		a.treeSearch.focusResults = false
	}
}

func (a *App) openWorkspaceHit(hit workspaceHit) {
	path := filepath.Join(a.root, hit.relative)
	if node := a.tree.at(path); node == nil {
		a.status = "文件已从工作区移走"
		return
	} else if err := a.checkTreeNode(node); err != nil {
		a.treeActionStatus(err)
		return
	}
	if convert.Detect(path) == convert.FormatUnknown {
		a.showWorkspaceTree()
		a.focusTreeEntry(path)
		return
	}
	a.openTreeFile(path)
	if !sameWorkspacePath(a.path, path) {
		return
	}
	a.revealWorkspaceFile()
	if hit.line != 0 {
		a.jumpWorkspaceMatch(hit.line)
	}
}

func workspaceLine(source string, target int) (line string, offset int, ok bool) {
	lineNumber := 1
	for value := range strings.SplitSeq(source, "\n") {
		if lineNumber == target {
			return value, offset, true
		}
		offset += len(value) + 1
		lineNumber++
	}
	return "", 0, false
}

func (a *App) jumpWorkspaceMatch(number int) {
	a.syncAll()
	query, err := a.treeSearch.query.compile()
	if err != nil {
		return
	}
	text := a.markdownSource()
	line, offset, ok := workspaceLine(text, number)
	if !ok {
		a.status = "文档已更新，请重新搜索"
		return
	}
	start, end, found := query.find(line)
	if !found {
		a.status = "原匹配已变化，定位第 " + itoa(number) + " 行"
	}
	at := offset + start
	end += offset
	if a.mode == ModePreview && a.source == nil {
		a.md.Anchor = "markdownx-workspace-match"
		a.md.AnchorVersion++
		a.md.AnchorBlock = func(document *parser.Document, name string) int {
			if name != "markdownx-workspace-match" {
				return -1
			}
			best := -1
			for i := 1; i < document.Len(); i++ {
				if b := document.Block(i); b.Start <= at && at <= b.End {
					best = i
				}
			}
			return best
		}
		return
	}
	if a.source == nil {
		if selection, ok := a.workspaceMatchSelection(text, at, end); ok {
			a.focusAt(selection)
			return
		}
		// Search also matches authored syntax that has no visible positions.
		a.toggleSource()
	}
	if a.source != nil {
		a.source.edit.SetSelections(sourceeditor.Selection{Anchor: at, Head: end})
		a.source.focus = true
	}
}

func (a *App) workspaceMatchSelection(source string, start, end int) (editor.Selection, bool) {
	_, ranges := a.visualSource()
	from := sourceCaret(a.core.Doc, source, ranges, start)
	to := sourceCaret(a.core.Doc, source, ranges, end)
	match := source[start:end]
	if from.Block == to.Block && from.Offset <= to.Offset {
		if visible := a.core.TextOf(from.Block); visible != nil && to.Offset <= visible.Len() &&
			visible.Text[from.Offset:to.Offset] == match {
			return editor.Selection{Anchor: from, Head: to}, true
		}
	}
	if match == "" {
		return editor.Selection{}, false
	}
	at := sort.Search(len(ranges), func(i int) bool { return ranges[i].End >= end })
	if at >= len(ranges) || ranges[at].Start > start {
		return editor.Selection{}, false
	}
	r := ranges[at]
	visible := a.core.TextOf(r.Block)
	if visible == nil {
		return editor.Selection{}, false
	}
	offsets := doc.InlineSourceOffsets(source[r.Start:r.End], visible.Text)
	first := sort.SearchInts(offsets, start-r.Start)
	last := first + utf8.RuneCountInString(match) - 1
	if last+1 >= len(offsets) || offsets[first] != start-r.Start {
		return editor.Selection{}, false
	}
	startByte, endByte := doc.RuneToByte(visible.Text, first), doc.RuneToByte(visible.Text, last+1)
	if offsets[last]+endByte-doc.RuneToByte(visible.Text, last) != end-r.Start ||
		visible.Text[startByte:endByte] != match {
		return editor.Selection{}, false
	}
	return editor.Selection{
		Anchor: editor.Pos{Block: r.Block, Offset: startByte},
		Head:   editor.Pos{Block: r.Block, Offset: endByte},
	}, true
}
