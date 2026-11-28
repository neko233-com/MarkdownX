package app

import (
	"errors"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown/parser"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type workspaceReference struct {
	path, markup string
	image        bool
}

var workspaceImageAltEscapes = strings.NewReplacer(`\`, `\\`, "[", `\[`, "]", `\]`)
var workspaceDropLabelSpaces = strings.NewReplacer("\r", " ", "\n", " ")

func (a *App) workspaceReference(path string, dir bool) (workspaceReference, bool) {
	var target string
	ref := workspaceReference{path: filepath.ToSlash(path)}
	if rel, ok := a.workspaceRelativePath(path); a.path != "" && ok && doc.RelativePathOK(filepath.ToSlash(rel)) {
		ref.path = filepath.ToSlash(rel)
		target = workspaceMarkdownPath(rel)
	} else {
		local := ref.path
		if !strings.HasPrefix(local, "/") {
			local = "/" + local
		}
		target = workspacePathEscapes.Replace((&url.URL{Scheme: "file", Path: local}).String())
		if _, ok := nativeFileURLPath(target); !ok {
			return workspaceReference{}, false
		}
	}
	name := workspaceDropLabelSpaces.Replace(filepath.Base(path))
	ref.image = !dir && (workspaceRasterPreview(path) || strings.EqualFold(filepath.Ext(path), ".svg"))
	if ref.image {
		ref.markup = "![" + workspaceImageAltEscapes.Replace(name) + "](<" + target + ">)"
	} else {
		ref.markup = "[" + doc.MarkdownInline(doc.PlainRich(name)) + "](<" + target + ">)"
	}
	return ref, true
}

func (a *App) documentDrop(e ui.Element) ([]workspaceReference, bool) {
	_, treeOver := ui.DragOver[*workspaceNode](e)
	_, nativeOver := ui.DataDragOver(e, workspaceFileDrop)
	legacyOver := e.FileDragOver()
	over := treeOver || nativeOver || legacyOver
	if node, dropped := ui.Drop[*workspaceNode](e); dropped {
		if err := a.checkDraggedTreeEntry(node); err != nil {
			a.treeActionStatus(err)
			return nil, over
		}
		if ref, ok := a.workspaceReference(node.path, node.isDir()); ok {
			return []workspaceReference{ref}, over
		}
		a.treeActionStatus(errors.New("无法引用拖入的路径"))
		return nil, over
	}
	paths := e.DroppedFiles()
	if drop, ok := ui.DropData(e, workspaceFileDrop); ok {
		var err error
		paths, err = drop.Data.Files()
		a.treeActionStatus(err)
	}
	var refs []workspaceReference
	var failure error
	for _, path := range paths {
		if !filepath.IsAbs(path) || strings.HasPrefix(filepath.VolumeName(path), `\\`) {
			continue
		}
		path = filepath.Clean(path)
		info, err := os.Stat(path)
		if err != nil {
			failure = err
			continue
		}
		if info.IsDir() || info.Mode().IsRegular() {
			if ref, ok := a.workspaceReference(path, info.IsDir()); ok {
				refs = append(refs, ref)
			}
		}
	}
	if failure == nil && len(paths) > 0 && len(refs) == 0 {
		failure = errors.New("无法引用拖入的文件或文件夹")
	}
	a.treeActionStatus(failure)
	return refs, over
}

func documentDropText(refs []workspaceReference, literal bool, separator string) string {
	var b strings.Builder
	for i, ref := range refs {
		if i > 0 {
			b.WriteString(separator)
		}
		if literal {
			b.WriteString(ref.path)
		} else {
			b.WriteString(ref.markup)
		}
	}
	return b.String()
}

type documentDropSyntax uint8

const (
	dropMarkdown documentDropSyntax = iota
	dropInlinePath
	dropBlockPath
)

func sourceDropSyntax(source string, at int) documentDropSyntax {
	parsed := parser.New(parser.Default | parser.ExtDollarMath)
	parsed.Set(source)
	for i := 1; i < parsed.Len(); i++ {
		block := parsed.Block(i)
		if at < block.Start || at > block.End {
			continue
		}
		if block.Kind == parser.CodeBlock || block.Kind == parser.MathBlock {
			if lines := parsed.LineCount(i); lines > 0 {
				_, first := parsed.Line(i, 0)
				lastLine, last := parsed.Line(i, lines-1)
				if first <= at && at <= last+len(lastLine) {
					return dropBlockPath
				}
			}
			continue
		}
		for _, span := range parsed.Inlines(i) {
			if span.Start < 0 || span.End > len(source) || at <= span.Start || at >= span.End {
				continue
			}
			markup := source[span.Start:span.End]
			var delimiter int
			switch span.Kind {
			case parser.Code:
				delimiter = len(markup) - len(strings.TrimLeft(markup, "`"))
			case parser.Math:
				delimiter = 1
				if strings.HasPrefix(markup, "$$") || strings.HasPrefix(markup, `\`) {
					delimiter = 2
				}
			default:
				continue
			}
			if span.Start+delimiter <= at && at <= span.End-delimiter {
				return dropInlinePath
			}
		}
	}
	return dropMarkdown
}

func (a *App) dropFilesInBlock(c *ui.Context, el ui.Element, b *doc.Block, p *blockBuffer) {
	refs, over := a.documentDrop(el)
	if over {
		el.Background(c.Theme().Selection)
		showWorkspaceDropHint(c, "插入到此处")
	}
	if len(refs) != 0 {
		a.pendingPane = a.documentTab
		x, y, _ := el.PointerPosition()
		if a.insertBlockDrop(b, p, ui.Point{X: x, Y: y}, refs) {
			el.Focus()
		}
	}
}

// insertBlockDrop reports whether an expanded inline editor should keep focus.
func (a *App) insertBlockDrop(b *doc.Block, p *blockBuffer, point ui.Point, refs []workspaceReference) bool {
	visibleAt := p.positionAt(point)
	a.endWorkspaceTemplateSession()
	a.syncAll()
	if a.dropPathInInlineSource(b, p, visibleAt, refs) {
		return true
	}
	at := clampOffset(b.Text.Text, a.modelOffset(b.ID, visibleAt))
	a.closeInlineSource(true)
	a.core.Sel = editor.Caret(editor.Pos{Block: b.ID, Offset: at})
	a.insertWorkspaceReferences(b, at, refs)
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
	return false
}

func emptyDropParagraph(b *doc.Block) bool {
	_, paragraph := b.Kind.(doc.TextKind)
	return paragraph && b.Text.IsEmpty() && len(b.Children) == 0
}

func (a *App) dropFilesOnDocument(c *ui.Context, el ui.Element, body ui.Rect) {
	refs, over := a.documentDrop(el)
	if over {
		showWorkspaceDropHint(c, "插入到此处")
	}
	if len(refs) == 0 {
		return
	}
	a.pendingPane = a.documentTab
	x, y, _ := el.PointerPosition()
	point := ui.Point{X: a.editBounds.X + x, Y: a.editBounds.Y + y}
	id, input := a.nearestTextInput(point)
	blocks := a.core.Doc.Blocks
	after := editor.Title
	insideBody := body.H == 0 || point.Y <= body.Y+body.H
	if len(blocks) != 0 {
		last := blocks[len(blocks)-1]
		after = last.ID
		if id == last.ID && emptyDropParagraph(last) {
			insideBody = true
		}
	}
	if input != nil && insideBody {
		local := ui.Point{X: point.X - input.bounds.X, Y: point.Y - input.bounds.Y}
		if a.insertBlockDrop(a.core.Block(id), input, local, refs) {
			a.focusReq = id
		}
		return
	}
	a.endWorkspaceTemplateSession()
	a.syncAll()
	a.closeInlineSource(true)
	insert := doc.MarkdownToBlocks(documentDropText(refs, false, "\n\n"), doc.Off{File: true})
	if !insert[len(insert)-1].Kind.HasText() {
		insert = append(insert, doc.TextBlock(""))
	}
	a.core.InsertBlocks(after, insert...)
	p := a.core.Sel.Head
	p.Offset = a.core.LenOf(p.Block)
	a.ResetBuffers()
	a.focusAt(editor.Caret(p))
}

func (a *App) insertWorkspaceReferences(b *doc.Block, at int, refs []workspaceReference) {
	_, code := b.Kind.(doc.CodeKind)
	marks := b.Text.MarksAt(at)
	if code || marks.Code {
		separator := " "
		if code {
			separator = "\n"
		}
		var literal doc.RichText
		literal.Push(documentDropText(refs, true, separator), marks)
		a.core.ReplaceSelection(literal)
		return
	}
	if emptyDropParagraph(b) && (refs[0].image || len(refs) > 1) {
		if a.core.PasteBlocks(doc.MarkdownToBlocks(documentDropText(refs, false, "\n\n"), doc.Off{File: true})) {
			return
		}
	}
	a.core.ReplaceSelection(doc.ParseInlineSource(documentDropText(refs, false, " ")))
}

func (a *App) dropPathInInlineSource(b *doc.Block, p *blockBuffer, at int, refs []workspaceReference) bool {
	r := a.inlineFor(b)
	if r == nil || at <= r.span.Start || at >= r.rawEnd() {
		return false
	}
	if off := at - r.span.Start; sourceDropSyntax(r.span.Markup, off) == dropMarkdown {
		return false
	}
	a.core.BreakCoalescing()
	caret := doc.ByteToRune(p.visible, at)
	p.native.SetSelection(caret, caret)
	a.pasteInlineSource(documentDropText(refs, true, " "))
	a.core.BreakCoalescing()
	a.services.Invalidate()
	return true
}
