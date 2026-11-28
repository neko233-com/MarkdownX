package app

import (
	"regexp"
	"strings"

	sourceeditor "github.com/egoist/mygo/ui/editor"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

// findHit is one match of the find query.
type findHit struct {
	block doc.BlockID
	start int // byte offsets into the block's text
	end   int
}

// findMatches lists every match of query in the document, case-insensitively.
func (a *App) findMatches(query string) []findHit {
	if strings.TrimSpace(query) == "" {
		return nil
	}
	q := regexp.MustCompile("(?i)" + regexp.QuoteMeta(query))
	var out []findHit
	a.core.Doc.ForEachBlock(func(b *doc.Block) {
		if b.Kind.HasText() {
			for _, at := range q.FindAllStringIndex(b.Text.Text, -1) {
				out = append(out, findHit{block: b.ID, start: at[0], end: at[1]})
			}
		}
	})
	return out
}

func (a *App) openFind(replace bool) {
	if a.source != nil {
		cmd := sourceeditor.Find
		if replace {
			cmd = sourceeditor.FindReplace
		}
		a.source.edit.Run(cmd)
		return
	}
	if a.mode == ModePreview {
		a.setMode(ModeEdit)
	}
	a.finding, a.findFocus, a.replacing = true, true, replace
	a.findKey = ""
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
	a.focusAt(editor.Selection{
		Anchor: editor.Pos{Block: hit.block, Offset: hit.start},
		Head:   editor.Pos{Block: hit.block, Offset: hit.end},
	})
}

// replaceAll swaps every match of the query for the replacement text.
func (a *App) replaceAll(with string) {
	a.syncAll()
	hits := a.findMatches(a.findQuery)
	if len(hits) == 0 {
		return
	}
	a.core.Checkpoint(editor.EditOther)
	// Replace from the end so earlier offsets stay valid.
	for i := len(hits) - 1; i >= 0; i-- {
		h := hits[i]
		a.core.ReplaceInBlock(h.block, h.start, h.end, with)
	}
	a.ResetBuffers()
	a.refreshMatches()
	a.status = "已全部替换"
}

// replaceOne swaps the current match for the replacement text.
func (a *App) replaceOne(with string) {
	a.syncAll()
	a.refreshMatches()
	if len(a.matches) == 0 {
		return
	}
	h := a.matches[a.matchIndex]
	a.core.Checkpoint(editor.EditOther)
	a.core.ReplaceInBlock(h.block, h.start, h.end, with)
	a.ResetBuffers()
	a.refreshMatches()
	a.jumpToMatch()
}
