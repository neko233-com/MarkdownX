package app

import (
	"bytes"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	sourceeditor "github.com/egoist/mygo/ui/editor"

	"github.com/neko233/markdownx/internal/convert"
	"github.com/neko233/markdownx/internal/doc"
)

type workspacePathMove struct {
	from, to string
}

type workspacePathMoves []workspacePathMove

func (m workspacePathMove) path(path string) string {
	if rel, moved := workspaceRel(m.from, path); moved {
		if m.to == "" {
			return ""
		}
		return filepath.Join(m.to, rel)
	}
	return path
}

func (moves workspacePathMoves) remap(path string) (string, workspacePathMove) {
	for _, move := range moves {
		if next := move.path(path); next != path {
			return next, move
		}
	}
	return path, workspacePathMove{}
}

func (moves workspacePathMoves) path(path string) string {
	target, _ := moves.remap(path)
	return target
}

type workspaceImageChange struct {
	failed        bool
	before, after []byte
	edits         []sourceeditor.Edit
	identity      os.FileInfo
}

func (moves workspacePathMoves) imageEdits(source, oldPath, newPath string) []sourceeditor.Edit {
	oldDir, newDir := filepath.Dir(oldPath), filepath.Dir(newPath)
	if sameWorkspacePath(oldDir, newDir) {
		return nil
	}
	var edits []sourceeditor.Edit
	for _, target := range doc.ImageDestinations(source) {
		if !doc.RelativePathOK(target.Path) {
			continue
		}
		path, err := url.PathUnescape(target.Path)
		if err != nil || !doc.RelativePathOK(path) {
			continue
		}
		oldRel := filepath.FromSlash(path)
		resolved := moves.path(filepath.Join(oldDir, oldRel))
		rel, err := filepath.Rel(newDir, resolved)
		if err != nil || sameWorkspacePath(rel, oldRel) {
			continue
		}
		edits = append(edits, sourceeditor.Edit{Start: target.Start, End: target.End, Text: workspaceMarkdownPath(rel)})
	}
	return edits
}

// Rebase the moved Markdown file and any Markdown documents actively edited
// under a moved folder. The other contents of that folder are moved verbatim.
func (a *App) rebaseWorkspaceImages(moves workspacePathMoves, undo map[string]workspaceImageChange) (map[string]workspaceImageChange, error) {
	paths := map[string]bool{}
	for _, move := range moves {
		if convert.Detect(move.from) == convert.FormatMarkdown {
			paths[move.from] = true
		}
	}
	for _, w := range a.workspaceWindows() {
		if w.file.path != "" && moves.path(w.path) != w.path {
			paths[w.path] = true
		}
	}
	for path := range undo {
		paths[path] = true
	}
	changes := make(map[string]workspaceImageChange)
	var problems error
	for oldPath := range paths {
		newPath, move := moves.remap(oldPath)
		if move.to == "" || sameWorkspacePath(filepath.Dir(oldPath), filepath.Dir(newPath)) {
			continue
		}
		if undo[oldPath].failed {
			// The disk paths still describe their original location.
			continue
		}
		change, err := moves.rewriteImageFile(move.to, oldPath, newPath, undo[oldPath])
		if err != nil {
			changes[newPath] = workspaceImageChange{failed: true}
			problems = errors.Join(problems, fmt.Errorf("%s: %w", filepath.Base(newPath), err))
			continue
		}
		if len(change.edits) != 0 {
			changes[newPath] = change
		}
	}
	return changes, problems
}

func (moves workspacePathMoves) rewriteImageFile(destination, oldPath, newPath string, undo workspaceImageChange) (workspaceImageChange, error) {
	info, err := os.Lstat(newPath)
	if err != nil {
		return workspaceImageChange{}, err
	}
	if !info.Mode().IsRegular() {
		return workspaceImageChange{}, nil
	}
	base, err := filepath.EvalSymlinks(destination)
	if err != nil {
		return workspaceImageChange{}, err
	}
	resolved, err := filepath.EvalSymlinks(newPath)
	if err != nil {
		return workspaceImageChange{}, err
	}
	rel, inside := workspaceRel(destination, newPath)
	if !inside || !sameWorkspacePath(resolved, filepath.Join(base, rel)) {
		return workspaceImageChange{}, errors.New("图片路径更新需要文档位于实际移动的目录内")
	}
	before, err := os.ReadFile(newPath)
	if err != nil {
		return workspaceImageChange{}, err
	}
	var after []byte
	var edits []sourceeditor.Edit
	if undo.after != nil && bytes.Equal(before, undo.after) {
		after = undo.before
		edits = reverseImageEdits(readMarkdownFile(oldPath, after).text, undo.edits)
	} else {
		source := readMarkdownFile(oldPath, before).text
		edits = moves.imageEdits(source, oldPath, newPath)
		if len(edits) == 0 {
			return workspaceImageChange{}, nil
		}
		if undo.after != nil {
			previous := readMarkdownFile(oldPath, undo.after).text
			reverse := reverseImageEdits(readMarkdownFile(newPath, undo.before).text, undo.edits)
			reuseImageEdits(source, edits, previous, reverse)
		}
		after = []byte(applyImageEdits(string(before), edits, true))
	}
	updated, err := replaceMarkdownFile(newPath, after, before, true)
	if err != nil {
		return workspaceImageChange{}, err
	}
	return workspaceImageChange{before: before, after: after, edits: edits, identity: updated}, nil
}

// Map the LF editor offsets back to the file's bytes when preserving its
// original BOM and line endings, including files with mixed line endings.
func applyImageEdits(text string, edits []sourceeditor.Edit, disk bool) string {
	var out strings.Builder
	previous, raw, logical := 0, 0, 0
	if disk && strings.HasPrefix(text, utf8BOM) {
		raw = len(utf8BOM)
	}
	seek := func(offset int) int {
		if !disk {
			return offset
		}
		for logical < offset {
			if text[raw] == '\r' && raw+1 < len(text) && text[raw+1] == '\n' {
				raw++
			}
			raw++
			logical++
		}
		return raw
	}
	for _, edit := range edits {
		start := seek(edit.Start)
		out.WriteString(text[previous:start])
		out.WriteString(edit.Text)
		previous = seek(edit.End)
	}
	out.WriteString(text[previous:])
	return out.String()
}

func reverseImageEdits(oldSource string, edits []sourceeditor.Edit) []sourceeditor.Edit {
	result := make([]sourceeditor.Edit, 0, len(edits))
	offset := 0
	for _, e := range edits {
		start := e.Start + offset
		result = append(result, sourceeditor.Edit{Start: start, End: start + len(e.Text), Text: oldSource[e.Start:e.End]})
		offset += len(e.Text) - (e.End - e.Start)
	}
	return result
}

// Keep the spelling of a matching destination when other parts of the
// document have changed. An angle-bracket destination may contain characters
// that cannot be restored verbatim in a bare destination.
func reuseImageEdits(source string, edits []sourceeditor.Edit, previous string, known []sourceeditor.Edit) {
	type target struct {
		text  string
		angle bool
	}
	key := func(s string, e sourceeditor.Edit) target {
		return target{s[e.Start:e.End], e.Start > 0 && s[e.Start-1] == '<' && e.End < len(s) && s[e.End] == '>'}
	}
	changes := make(map[target]string, len(known))
	for _, e := range known {
		k := key(previous, e)
		if existing, ok := changes[k]; ok && existing != e.Text {
			changes[k] = "" // The earlier destinations had different spellings.
		} else {
			changes[k] = e.Text
		}
	}
	for i := range edits {
		if text := changes[key(source, edits[i])]; text != "" {
			edits[i].Text = text
		}
	}
}

func imageRebaseOffset(offset int, edits []sourceeditor.Edit) int {
	delta := 0
	for _, e := range edits {
		if offset < e.Start {
			break
		}
		if offset < e.End {
			return delta + e.Start + min(offset-e.Start, len(e.Text))
		}
		delta += len(e.Text) - (e.End - e.Start)
	}
	return offset + delta
}

func (a *App) rebaseWorkspaceEditor(moves workspacePathMoves, path string, change workspaceImageChange) {
	a.closeInlineSource(true)
	a.updateDirty()
	oldPath := a.path
	oldSource := a.markdownSource()
	wasDirty := a.dirty
	correspondsToDisk := change.after != nil && bytes.Equal(a.file.disk, change.before)
	edits := change.edits
	if !correspondsToDisk || oldSource != a.file.text {
		edits = moves.imageEdits(oldSource, oldPath, path)
		if len(edits) > 0 && len(change.edits) > 0 {
			reuseImageEdits(oldSource, edits, readMarkdownFile(oldPath, change.before).text, change.edits)
		}
	}
	if len(edits) > 0 {
		a.applyImageRebaseToEditor(oldSource, edits)
	}
	if correspondsToDisk {
		a.file = readMarkdownFile(path, change.after)
		a.file.identity = change.identity
		a.savedText = a.file.text
		if !wasDirty {
			a.markSaved()
		}
	} else {
		a.file.path = path
	}
	a.updateDirty()
}

func (a *App) applyImageRebaseToEditor(before string, edits []sourceeditor.Edit) {
	if s := a.source; s != nil {
		selections := s.edit.Selections()
		for i := range selections {
			selections[i].Anchor = imageRebaseOffset(selections[i].Anchor, edits)
			selections[i].Head = imageRebaseOffset(selections[i].Head, edits)
		}
		s.edit.Buffer.Edit(edits...)
		s.edit.SetSelections(selections...)
		return
	}
	_, ranges := a.visualSource()
	caret := a.core.Sel.Head
	at := 0
	if path, ok := a.core.Path(caret.Block); ok && len(path) > 0 {
		root := a.core.Doc.Blocks[path[0].Index].ID
		for _, r := range ranges {
			if r.Block == root {
				at = r.Start
				if b := a.core.Block(caret.Block); b != nil {
					if offset := strings.Index(before[r.Start:r.End], b.Text.Text); offset >= 0 {
						at += offset + min(caret.Offset, len(b.Text.Text))
					}
				}
				break
			}
		}
	}
	next := applyImageEdits(before, edits, false)
	d := doc.ReadSource(next)
	_, newRanges := d.WorkspaceMarkdown()
	p := sourceCaret(d, next, newRanges, imageRebaseOffset(at, edits))
	a.core.ReplaceDocument(d, p)
	a.focus, a.focusReq = p.Block, 0
	a.ResetBuffers()
	if b := a.core.Block(p.Block); b != nil {
		at := doc.ByteToRune(b.Text.Text, p.Offset)
		a.restoreSelection(p.Block, at, at)
	}
}

func (a *App) reportWorkspaceImageChanges(status string, err error) {
	a.status = status
	if err != nil {
		a.status += " · 部分图片路径未能保存：" + strings.ReplaceAll(err.Error(), "\n", "，")
	}
}
