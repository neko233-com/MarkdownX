package app

import (
	"strconv"
	"strings"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

// detectMarkdownPrefix reads the Typora-style marker at the start of a line:
// "# ", "- ", "1. ", "> ", "- [ ] " or "```". It returns the block kind to
// switch to, the text left after the marker, and whether the caret belongs in
// a child block (a quote holds its text in children).
func detectMarkdownPrefix(text string) (kind doc.BlockKind, rest string, child bool, ok bool) {
	// Headings: one to six '#' then a space.
	if h := strings.TrimLeft(text, "#"); len(text) > len(h) {
		level := len(text) - len(h)
		if level <= 6 && strings.HasPrefix(h, " ") {
			return doc.HeadingKind{Level: uint8(level)}, h[1:], false, true
		}
	}
	// Task list: "- [ ] " and "- [x] ".
	lower := strings.ToLower(text)
	for _, p := range []string{"- [ ] ", "* [ ] ", "- [x] ", "* [x] "} {
		if strings.HasPrefix(lower, p) {
			return doc.TodoKind{Done: strings.Contains(p, "x]")}, text[len(p):], false, true
		}
	}
	// Quote: "> ".
	if strings.HasPrefix(text, "> ") {
		return doc.QuoteKind{}, text[2:], true, true
	}
	// Bullets: "- ", "* ", "+ ".
	for _, p := range []string{"- ", "* ", "+ "} {
		if strings.HasPrefix(text, p) {
			return doc.BulletKind{}, text[len(p):], false, true
		}
	}
	// Ordered list: digits then ". ".
	if n := leadingDigits(text); n > 0 && len(text) > n+1 && text[n:n+2] == ". " {
		seq := uint32(1)
		if v, err := strconv.ParseUint(text[:n], 10, 32); err == nil && v > 0 {
			seq = uint32(v)
		}
		return doc.OrderedKind{Seq: &seq}, text[n+2:], false, true
	}
	// Fenced code: "```" with an optional language.
	if strings.HasPrefix(text, "```") {
		lang := strings.TrimSpace(text[3:])
		code := ""
		if i := strings.IndexByte(lang, '\n'); i >= 0 {
			code = lang[i+1:]
			lang = strings.TrimSpace(lang[:i])
		}
		return doc.CodeKind{Lang: lang}, code, false, true
	}
	return nil, text, false, false
}

// leadingDigits counts the digits at the start of s.
func leadingDigits(s string) int {
	i := 0
	for i < len(s) && s[i] >= '0' && s[i] <= '9' {
		i++
	}
	return i
}

// autoFormat switches a paragraph to the block its Markdown marker names, the
// way Typora does while you type. caret is the byte offset in the buffer; the
// marker only counts when it sits right before the caret.
func (a *App) autoFormat(b *doc.Block, caret int) bool {
	text := b.Text.Text
	if caret != len(text) {
		return false
	}
	// A bullet that gains "[ ] " turns into a task item: typing "- [ ] "
	// makes a bullet first, then the rest of the marker.
	if _, isBullet := b.Kind.(doc.BulletKind); isBullet {
		lower := strings.ToLower(text)
		for _, p := range []string{"[ ] ", "[x] "} {
			if !strings.HasPrefix(lower, p) {
				continue
			}
			a.afterFormat(a.core.FormatPrefix(b.ID, doc.TodoKind{Done: strings.Contains(p, "x")}, len(p), text[len(p):]))
			return true
		}
		return false
	}
	if _, ok := b.Kind.(doc.TextKind); !ok {
		return false
	}
	kind, rest, _, ok := detectMarkdownPrefix(text)
	if !ok {
		return false
	}
	if _, code := kind.(doc.CodeKind); code {
		return false
	}
	a.afterFormat(a.core.FormatPrefix(b.ID, kind, len(text)-len(rest), rest))
	return true
}

func (a *App) autoInline(b *doc.Block, caret int) {
	if caret != b.Text.Len() || doc.DefinitionDraft(b.Text.Text) {
		return
	}
	switch b.Kind.(type) {
	case doc.CodeKind, doc.DefinitionKind:
		return
	}
	for _, s := range b.Text.Spans() {
		if s.End != caret || !doc.MarksEqual(&s.Marks, &doc.Marks{}) {
			continue
		}
		if text, ok := doc.CompleteInline(b.Text.Text[s.Start:s.End]); ok {
			a.afterFormat(a.core.FormatInline(b.ID, s.Start, s.End, text))
			a.bufs[b.ID].plainTyping = true
		}
		break
	}
}

func (a *App) formatOnEnter(b *doc.Block) bool {
	if _, text := b.Kind.(doc.TextKind); !text || len(b.Children) != 0 ||
		!a.core.Sel.Collapsed() || a.core.Sel.Head.Offset != b.Text.Len() {
		return false
	}
	text := b.Text.Text
	if doc.IsTOCMarker(text) {
		a.afterFormat(a.core.ReplaceBlock(b.ID, doc.NewBlock(doc.TOCKind{Marker: strings.TrimSpace(text)}, doc.RichText{}), true))
		return true
	}
	if definition := doc.DefinitionBlock(text); definition != nil {
		a.afterFormat(a.core.ReplaceBlock(b.ID, definition, true))
		return true
	}
	if text == "$$" || text == `\[` {
		a.openEquationFor(b)
		return true
	}
	if kind, rest, _, ok := detectMarkdownPrefix(text); ok {
		if _, code := kind.(doc.CodeKind); code {
			a.afterFormat(a.core.FormatPrefix(b.ID, kind, len(text)-len(rest), rest))
			return true
		}
	}
	if table := doc.TableFromHeader(text); table != nil {
		p := a.core.ReplaceBlock(b.ID, table, true)
		first := table.Kind.(doc.TableKind).Table.Rows[1][0].Blocks[0]
		p.Block = first.ID
		a.afterFormat(p)
		return true
	}
	if len(text) >= 3 {
		switch {
		case strings.Trim(text, "-") == "", strings.Trim(text, "*") == "", strings.Trim(text, "_") == "":
			a.afterFormat(a.core.ReplaceBlock(b.ID, doc.NewBlock(doc.DividerKind{}, doc.RichText{}), true))
			return true
		}
	}
	return false
}

// afterFormat moves the caret into the block the marker produced.
func (a *App) afterFormat(p editor.Pos) {
	a.core.Sel = editor.Caret(p)
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
}

// insertBelow adds an empty paragraph under the focused block, as Typora's
// Ctrl+Enter does.
func (a *App) insertBelow() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	nb := doc.TextBlock("")
	a.core.InsertBlocks(b.ID, nb)
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
}
