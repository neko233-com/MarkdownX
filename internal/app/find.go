package app

import (
	"strings"
	"unicode/utf8"

	"github.com/neko233/markdownx/internal/doc"
)

// findHit is one match of the find query.
type findHit struct {
	block doc.BlockID
	start int // byte offsets into the block's text
	end   int
}

// findMatches lists every match of query in the document, case-insensitively.
func (a *App) findMatches(query string) []findHit {
	q := strings.ToLower(strings.TrimSpace(query))
	if q == "" {
		return nil
	}
	var out []findHit
	for _, b := range a.core.Doc.Blocks {
		collectHits(b, q, &out)
	}
	return out
}

func collectHits(b *doc.Block, query string, out *[]findHit) {
	if b.Kind.HasText() {
		text := strings.ToLower(b.Text.Text)
		from := 0
		for {
			k := strings.Index(text[from:], query)
			if k < 0 {
				break
			}
			at := from + k
			*out = append(*out, findHit{block: b.ID, start: at, end: at + len(query)})
			// Step one rune so overlapping matches are still found.
			_, size := utf8.DecodeRuneInString(text[at:])
			if size < 1 {
				size = 1
			}
			from = at + size
		}
	}
	for _, c := range b.Children {
		collectHits(c, query, out)
	}
	if tk, ok := b.Kind.(doc.TableKind); ok && tk.Table != nil {
		for r := range tk.Table.Rows {
			for c := range tk.Table.Rows[r] {
				for _, cb := range tk.Table.Rows[r][c].Blocks {
					collectHits(cb, query, out)
				}
			}
		}
	}
}

// refreshMatches recomputes the hits when the query or the document changed.
func (a *App) refreshMatches() {
	key := a.findQuery + "\x00" + itoa(int(a.core.Generation))
	if key == a.findKey {
		return
	}
	a.findKey = key
	a.matches = a.findMatches(a.findQuery)
	if a.matchIndex >= len(a.matches) {
		a.matchIndex = 0
	}
	a.jumpToMatch()
}

// jumpToMatch selects and focuses the current match.
func (a *App) jumpToMatch() {
	if len(a.matches) == 0 {
		return
	}
	i := a.matchIndex
	if i < 0 || i >= len(a.matches) {
		i = 0
	}
	hit := a.matches[i]
	b := a.core.Block(hit.block)
	if b == nil {
		return
	}
	text := b.Text.Text
	a.pending[hit.block] = [2]int{doc.ByteToRune(text, hit.start), doc.ByteToRune(text, hit.end)}
	a.focusReq = hit.block
}

// replaceAll swaps every match of the query for the replacement text.
func (a *App) replaceAll(with string) {
	a.syncAll()
	hits := a.matchMatches()
	if len(hits) == 0 {
		return
	}
	// Replace from the end so earlier offsets stay valid.
	for i := len(hits) - 1; i >= 0; i-- {
		h := hits[i]
		b := a.core.Block(h.block)
		if b == nil {
			continue
		}
		b.Text.Replace(h.start, h.end, doc.PlainRich(with))
	}
	a.core.Dirty = true
	a.core.Generation++
	a.ResetBuffers()
	a.findKey = ""
	a.refreshMatches()
	a.status = "已全部替换"
}

// replaceOne swaps the current match for the replacement text.
func (a *App) replaceOne(with string) {
	a.syncAll()
	if len(a.matches) == 0 {
		return
	}
	h := a.matches[a.matchIndex]
	b := a.core.Block(h.block)
	if b == nil {
		return
	}
	b.Text.Replace(h.start, h.end, doc.PlainRich(with))
	a.core.Dirty = true
	a.core.Generation++
	a.ResetBuffers()
	a.findKey = ""
	a.refreshMatches()
}

// matchMatches is the live hit list, recomputed when it is stale.
func (a *App) matchMatches() []findHit {
	return a.findMatches(a.findQuery)
}
