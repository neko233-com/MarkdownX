package app

import (
	"strconv"
	"strings"
	"unicode/utf8"

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
			a.core.Checkpoint(editor.EditOther)
			b.Kind = doc.TodoKind{Done: strings.Contains(p, "x")}
			b.Text = doc.PlainRich(text[len(p):])
			a.afterFormat(b)
			return true
		}
		return false
	}
	if _, ok := b.Kind.(doc.TextKind); !ok {
		return false
	}
	kind, rest, child, ok := detectMarkdownPrefix(text)
	if !ok {
		return false
	}
	a.core.Checkpoint(editor.EditOther)
	target := b
	if child {
		nb := doc.TextBlock(rest)
		b.Kind = doc.QuoteKind{}
		b.Text = doc.RichText{}
		b.Children = append([]*doc.Block{nb}, b.Children...)
		target = nb
	} else {
		b.Kind = kind
		b.Text = doc.PlainRich(rest)
	}
	a.afterFormat(target)
	return true
}

// afterFormat moves the caret into the block the marker produced.
func (a *App) afterFormat(target *doc.Block) {
	a.core.Generation++
	a.dirty = true
	a.focus = target.ID
	a.focusReq = target.ID
	a.ResetBuffers()
	n := utf8.RuneCountInString(target.Text.Text)
	a.restoreSelection(target.ID, n, n)
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
	a.core.InsertAfter(b.ID, nb)
	a.focus = nb.ID
	a.focusReq = nb.ID
	a.ResetBuffers()
	a.restoreSelection(nb.ID, 0, 0)
}

// openLink asks for a URL to mark the selection with.
func (a *App) openLink() {
	a.linkOpen = true
	a.linkFocus = true
	a.linkURL = ""
}

// doInsertLink applies the typed URL as a link mark over the selection, or
// arms it for the next typed text when nothing is selected.
func (a *App) doInsertLink() {
	a.linkOpen = false
	url := strings.TrimSpace(a.linkURL)
	if url == "" {
		a.status = "链接地址不能为空"
		return
	}
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	lo, hi := a.selectionOf(b)
	a.core.Sel = editor.Selection{
		Anchor: editor.Pos{Block: b.ID, Offset: lo},
		Head:   editor.Pos{Block: b.ID, Offset: hi},
	}
	a.core.ToggleMark(
		func(m *doc.Marks) bool { return m.Link != nil },
		func(m *doc.Marks, on bool) {
			if on {
				u := url
				m.Link = &u
			} else {
				m.Link = nil
			}
		},
	)
	a.ResetBuffers()
	a.restoreSelection(b.ID, doc.ByteToRune(b.Text.Text, lo), doc.ByteToRune(b.Text.Text, hi))
	a.status = "已插入链接"
}
