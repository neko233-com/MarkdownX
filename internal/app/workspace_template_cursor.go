package app

import (
	"strings"

	sourceeditor "github.com/egoist/mygo/ui/editor"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func (a *App) createWorkspaceTemplate(d *workspaceDialog) error {
	name, err := workspaceEntryName(d.name)
	if err != nil {
		return err
	}
	name = workspaceMarkdownName(name)
	out, err := d.templates[d.templateChoice].render(name, &d.templateValues)
	if err != nil {
		return err
	}
	path, err := a.createTreeEntry(d.dir, name, false, out.text)
	if err == nil && sameWorkspacePath(a.path, path) {
		if len(out.fields) > 0 {
			a.startWorkspaceTemplateSession(out)
		} else if out.cursor >= 0 {
			a.focusWorkspaceTemplateCursor(templateLFOffset(out.text, out.cursor))
		}
	}
	return err
}

func templateLFOffset(raw string, at int) int {
	prefix := strings.TrimPrefix(raw[:at], utf8BOM)
	return len(prefix) - strings.Count(prefix, "\r\n")
}

func (a *App) focusWorkspaceTemplateCursor(at int) {
	a.treeFocus, a.treeTarget = false, ""
	text, ranges := a.visualSource()
	at = clampOffset(text, at)
	if a.mode != ModePreview {
		if p, ok := a.workspaceTemplateVisualCursor(text, ranges, at); ok {
			a.focusAt(editor.Caret(p))
			return
		}
	}
	// Authored syntax and whitespace can have no position in a visual block.
	a.toggleSource()
	if a.source != nil {
		a.source.edit.SetSelections(sourceeditor.Selection{Anchor: at, Head: at})
	}
}

func (a *App) workspaceTemplateVisualCursor(text string, ranges []doc.SourceRange, at int) (editor.Pos, bool) {
	if len(ranges) == 0 {
		return editor.Pos{Block: a.core.Doc.Blocks[0].ID}, true
	}
	if p, ok := a.workspaceTemplateMappedCaret(text, ranges, at); ok {
		return p, true
	}
	last := ranges[len(ranges)-1]
	if at > last.End && strings.TrimSpace(text[last.End:]) == "" {
		if strings.Count(text[last.End:at], "\n") < 2 {
			if visible := a.core.TextOf(last.Block); visible != nil {
				return editor.Pos{Block: last.Block, Offset: visible.Len()}, true
			}
		} else {
			// Empty paragraphs are omitted by Markdown export until edited.
			id := a.core.InsertBlockAfter(last.Block, doc.TextBlock(""))
			a.ResetBuffers()
			a.checkSavedText()
			return editor.Pos{Block: id}, true
		}
	}
	return editor.Pos{}, false
}

func (a *App) workspaceTemplateMappedCaret(text string, ranges []doc.SourceRange, at int) (editor.Pos, bool) {
	for _, r := range ranges {
		if at < r.Start || at > r.End {
			continue
		}
		if visible := a.core.TextOf(r.Block); visible != nil {
			start := strings.Index(text[r.Start:r.End], visible.Text)
			if start >= 0 {
				offset := at - r.Start - start
				if offset >= 0 && offset <= visible.Len() {
					return editor.Pos{Block: r.Block, Offset: offset}, true
				}
			}
		}
		break
	}
	return editor.Pos{}, false
}
