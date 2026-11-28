package app

import (
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"

	"github.com/neko233/markdownx/internal/convert"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func (a *App) followDocumentLink(target string) {
	if strings.HasPrefix(strings.ToLower(target), "www.") {
		target = "https://" + target
	}
	if ui.SafeToOpen(target) {
		a.services.OpenURL(target)
		return
	}
	raw, anchor, _ := strings.Cut(target, "#")
	if text, err := url.PathUnescape(anchor); err == nil {
		anchor = text
	}
	if raw == "" && anchor != "" {
		a.followHeading(anchor)
		return
	}
	path, info := a.linkedDocument(raw)
	if info == nil {
		a.status = "无法打开链接指向的文档"
		return
	}
	if info.IsDir() {
		a.openWorkspaceFolder(path)
		return
	}
	if convert.Detect(path) == convert.FormatUnknown {
		if a.win != nil {
			mygo.Shell.ShowItemInFolder(path)
		}
		return
	}
	if a.path != "" && samePath(a.path, path) {
		a.followHeading(anchor)
		return
	}
	var destination *App
	if a.windows != nil {
		destination = a.windows.activateFile(path)
	} else {
		a.OpenFile(path)
		destination = a
	}
	if destination != nil && samePath(destination.path, path) {
		destination.followHeading(anchor)
		destination.services.Invalidate()
	}
}

func (a *App) linkedDocument(raw string) (string, os.FileInfo) {
	path, absolute := nativeFileURLPath(raw)
	if !absolute {
		if !doc.RelativePathOK(raw) {
			return "", nil
		}
		rel, err := url.PathUnescape(raw)
		if err != nil || !doc.RelativePathOK(rel) {
			return "", nil
		}
		dir := a.root
		if a.path != "" {
			dir = filepath.Dir(a.path)
		}
		if dir == "" {
			return "", nil
		}
		path = filepath.Join(dir, filepath.FromSlash(rel))
	}
	options := []string{path}
	if filepath.Ext(path) == "" {
		options = append(options, path+".md", path+".markdown")
	}
	for _, candidate := range options {
		if info, err := os.Stat(candidate); err == nil && (info.IsDir() || info.Mode().IsRegular()) {
			return candidate, info
		}
	}
	return "", nil
}

func nativeFileURLPath(link string) (string, bool) {
	path, ok := doc.LocalFileURLPath(link)
	if !ok {
		return "", false
	}
	if runtime.GOOS == "windows" && len(path) > 3 && path[2] == ':' && path[3] == '/' {
		path = path[1:]
	}
	path = filepath.FromSlash(path)
	return path, filepath.IsAbs(path)
}

func (a *App) followHeading(anchor string) {
	if anchor == "" {
		return
	}
	d := a.core.Doc
	if a.source != nil {
		d = a.source.document()
	}
	for _, h := range d.Headings(markdown.Slug) {
		if strings.EqualFold(anchor, h.Slug) {
			a.jumpHeading(d, h.Block)
			return
		}
	}
	a.status = "找不到链接指向的标题"
}

func (a *App) inlineLinkClick(b *doc.Block, p *blockBuffer, target string) {
	if p.pointerMods&ui.Cmd != 0 {
		a.pendingLink = target
		return
	}
	if p.pointerMods&ui.Shift != 0 {
		return
	}
	at := a.modelOffset(b.ID, p.positionAt(p.pointerPoint))
	for _, span := range b.Text.Spans() {
		if span.Marks.Link != nil && *span.Marks.Link == target && span.Start <= at && at <= span.End {
			if at == span.End {
				at = doc.RuneToByte(b.Text.Text, doc.ByteToRune(b.Text.Text, at)-1)
			}
			a.inlineClick = editor.Pos{Block: b.ID, Offset: at}
			return
		}
	}
}

func (p *blockBuffer) inlineObjectAt(point ui.Point) (int, bool) {
	for _, box := range p.native.VisibleParagraphs() {
		if point.Y < box.Top || point.Y >= box.Top+box.Height {
			continue
		}
		shape := p.layout(box.Index, p.bounds.W)
		if shape == nil {
			continue
		}
		for _, object := range shape.Objects() {
			if object.Rect.Contains(point.X, point.Y-box.Top) {
				start := doc.RuneToByte(p.visible, box.Start)
				return start + utf16Offset(p.visible[start:], object.Index), true
			}
		}
	}
	return 0, false
}

func (a *App) observeInlinePointer(b *doc.Block, p *blockBuffer, ev ui.InputEvent) {
	if ev.Button != 0 {
		return
	}
	switch ev.Kind {
	case ui.InputPointerDown:
		p.pointerDown, p.pointerMods, p.pointerPoint = true, ev.Mods, ui.Point{X: ev.X, Y: ev.Y}
	case ui.InputPointerUp:
		down := p.pointerDown
		p.pointerDown = false
		dx, dy := ev.X-p.pointerPoint.X, ev.Y-p.pointerPoint.Y
		if !down || ev.Canceled || dx*dx+dy*dy >= 16 {
			return
		}
		if at, ok := p.inlineObjectAt(ui.Point{X: ev.X, Y: ev.Y}); ok {
			at = a.modelOffset(b.ID, at)
			switch p.pointerMods {
			case ui.Cmd:
				if ref, ok := referenceAtPosition(b.Text, at); ok {
					a.followReference(ref, editor.Pos{Block: b.ID, Offset: at})
				}
			case 0:
				a.inlineClick = editor.Pos{Block: b.ID, Offset: at}
			}
		}
	}
}

// openLink places the caret in the destination of the link as Markdown.
// The template itself is a view change until the user types.
func (a *App) openLink() {
	b := a.focused()
	if b == nil || !b.Kind.HasText() || a.core.Sel.AcrossBlocks() {
		return
	}
	switch b.Kind.(type) {
	case doc.CodeKind, doc.DefinitionKind:
		return
	}
	a.syncAll()
	if edit := a.inlineFor(b); edit != nil && a.editLinkedDefinition(b, edit.span.Start) {
		return
	}
	a.closeInlineSource(true)
	lo, hi := a.selectionOf(b)
	if lo < hi && b.Text.Text[lo:hi] == doc.Obj && a.editLinkedDefinition(b, lo) {
		return
	}
	for _, s := range b.Text.Spans() {
		if s.Marks.Link == nil || s.Start > lo || s.End < hi {
			continue
		}
		offset := min(lo, editor.PrevBoundary(b.Text.Text, s.End))
		if source, found := b.Text.InlineSourceAt(offset, true); found {
			if start, end, ok := linkDestinationSelection(source.Markup, *s.Marks.Link); ok {
				a.requestInlineSource(b, source, start, end)
				return
			}
		}
		break
	}
	label := doc.MarkdownInline(b.Text.Slice(lo, hi))
	markup := "[" + label + "]()"
	at := len(markup) - 1
	if lo == hi {
		at = 1
	}
	a.requestInlineSource(b, doc.InlineSource{Start: lo, End: hi, Markup: markup}, at, at)
}

func (a *App) editLinkedDefinition(b *doc.Block, offset int) bool {
	ref, ok := referenceAtPosition(b.Text, offset)
	if !ok || ref.Footnote {
		return false
	}
	if link, ok := a.references().Link(ref.Label); ok {
		if definition := a.core.Block(link.Block); definition != nil {
			lo, hi, _ := doc.LinkDefinitionDestination(definition.Text.Text)
			a.focusAt(editor.Selection{
				Anchor: editor.Pos{Block: link.Block, Offset: lo},
				Head:   editor.Pos{Block: link.Block, Offset: hi},
			})
			return true
		}
	}
	return false
}

func linkDestinationSelection(raw, target string) (int, int, bool) {
	for at := 0; at < len(raw); {
		tail, ok := strings.CutPrefix(raw[at:], "](")
		if ok {
			trimmed := strings.TrimLeft(tail, " \t\n<")
			if strings.HasPrefix(trimmed, target) {
				start := len(raw) - len(trimmed)
				return start, start + len(target), true
			}
		}
		if n := strings.Index(raw[at+1:], "]("); n >= 0 {
			at += n + 1
		} else {
			break
		}
	}
	return 0, 0, false
}

func (a *App) requestInlineSource(b *doc.Block, source doc.InlineSource, start, end int) {
	a.focusAt(editor.Caret(editor.Pos{Block: b.ID, Offset: source.Start}))
	a.inlineRequest = &inlineSourceRequest{block: b.ID, source: source, start: start, end: end}
	a.services.Invalidate()
}
