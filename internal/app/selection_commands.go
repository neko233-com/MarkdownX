package app

import (
	"strings"
	"unicode"
	"unicode/utf8"

	sourceeditor "github.com/egoist/mygo/ui/editor"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type textRange uint8

const (
	paragraphRange textRange = iota
	sentenceRange
	styleRange
)

func (a *App) editTextRange(kind textRange, remove bool) {
	if a.source != nil {
		a.editSourceRange(kind, remove)
		return
	}
	if _, table := a.core.TableCellAt(a.focus); table && !remove && kind != paragraphRange {
		a.selectTableCell(kind == sentenceRange)
		return
	}
	a.editFocused(func(b *doc.Block) bool {
		at := clampOffset(b.Text.Text, a.core.Sel.Head.Offset)
		if kind == paragraphRange && remove {
			a.core.DeleteBlock(b.ID)
			return true
		}
		start, end := 0, b.Text.Len()
		switch kind {
		case sentenceRange:
			start, end = lineBounds(b.Text.Text, at)
			if _, code := b.Kind.(doc.CodeKind); !code {
				start, end = sentenceBounds(b.Text.Text, at, start, end, remove)
			}
		case styleRange:
			start, end = styledBounds(&b.Text, at)
		}
		if start == end {
			return false
		}
		a.core.Sel = editor.Selection{Anchor: editor.Pos{Block: b.ID, Offset: start}, Head: editor.Pos{Block: b.ID, Offset: end}}
		a.core.BreakCoalescing()
		a.core.PendingMk = nil
		if remove {
			a.core.ReplaceSelection(doc.RichText{})
		}
		return true
	})
}

func (a *App) editSourceRange(kind textRange, remove bool) {
	e := &a.source.edit
	text, at := a.source.text(), e.Selection().Head
	start, end := lineBounds(text, at)
	if kind == paragraphRange && strings.TrimSpace(text[start:end]) != "" {
		for start > 0 {
			prior, _ := lineBounds(text, start-1)
			if strings.TrimSpace(text[prior:start-1]) == "" {
				break
			}
			start = prior
		}
		for end < len(text) {
			_, next := lineBounds(text, end+1)
			if strings.TrimSpace(text[end+1:next]) == "" {
				break
			}
			end = next
		}
	}
	if remove {
		if end < len(text) {
			end++
		} else if start > 0 {
			start--
		}
	}
	if start == end {
		return
	}
	a.applySourceRange(start, end, remove)
}

func (a *App) applySourceRange(start, end int, remove bool) {
	e := &a.source.edit
	e.SetSelections(sourceeditor.Selection{Anchor: start, Head: end})
	if remove {
		e.Run(sourceeditor.Backspace)
		a.updateDirty()
	}
	a.source.focus = true
}

func lineBounds(s string, at int) (start, end int) {
	at = clampOffset(s, at)
	start = strings.LastIndexByte(s[:at], '\n') + 1
	end = len(s)
	if next := strings.IndexByte(s[at:], '\n'); next >= 0 {
		end = at + next
	}
	return start, end
}

func sentenceBounds(s string, at, start, end int, remove bool) (int, int) {
	low, high := start, end
	for i, r := range s[start:end] {
		if !strings.ContainsRune(".!?。！？", r) {
			continue
		}
		next := start + i + utf8.RuneLen(r)
		following := strings.TrimLeft(s[next:end], ".!?。！？”’»「」』\"')")
		next = end - len(following)
		space := strings.TrimLeftFunc(following, unicode.IsSpace)
		if r == '.' && following != "" && space == following {
			continue // A decimal or an internal dot without a boundary.
		}
		if at <= next || at < end-len(space) {
			high = next
			if remove {
				high = end - len(space)
			}
			break
		}
		low = end - len(space)
	}
	if low < high {
		low = high - len(strings.TrimLeftFunc(s[low:high], unicode.IsSpace))
	}
	return min(low, high), high
}

func styledBounds(text *doc.RichText, at int) (int, int) {
	var plain doc.Marks
	for _, span := range text.Spans() {
		if at >= span.Start && at <= span.End && !doc.MarksEqual(&span.Marks, &plain) {
			return span.Start, span.End
		}
	}
	return at, at
}
