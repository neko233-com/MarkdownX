package app

import (
	"math"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/go-text/typesetting/segmenter"
)

// dropCaret uses the caret positions laid out by the source editor, including
// wraps, tabs, glyph widths and scrolling.
func (s *sourceView) dropCaret(x, y float32) (int, ui.Rect, bool) {
	if s.edit.ViewportHeight() <= 0 {
		return 0, ui.Rect{}, false
	}
	snapshot := s.edit.Buffer.Snapshot()
	contentY := float64(y) + s.edit.ScrollY()
	count := snapshot.LineCount()
	line := min(count-1, sort.Search(count, func(i int) bool {
		return s.edit.LineTop(i+1) > contentY
	}))
	text, start := snapshot.Line(line), snapshot.LineStart(line)
	rectAt := func(i int) ui.Rect {
		for i > 0 && i < len(text) && !utf8.RuneStart(text[i]) {
			i--
		}
		r, _ := s.edit.Rect(start + i)
		return r
	}
	last := rectAt(len(text))
	if last.H == 0 {
		return 0, ui.Rect{}, false
	}
	y = min(y, last.Y+last.H/2)
	lo := sort.Search(len(text)+1, func(i int) bool {
		r := rectAt(i)
		return y < r.Y+r.H
	})
	caret := rectAt(lo)
	hi := sort.Search(len(text)+1, func(i int) bool { return rectAt(i).Y > caret.Y })
	chosen := lo
	distance := math.Abs(float64(x - caret.X))
	consider := func(i int) {
		r := rectAt(i)
		if d := math.Abs(float64(x - r.X)); d < distance {
			chosen, caret, distance = i, r, d
		}
	}
	row := text[lo:min(hi, len(text))]
	if strings.IndexFunc(row, func(r rune) bool { return r >= utf8.RuneSelf }) < 0 {
		// X is monotonic for ASCII, even on a very long unwrapped line.
		i := lo + sort.Search(hi-lo, func(i int) bool { return rectAt(lo+i).X >= x })
		if i > lo {
			consider(i - 1)
		}
		if i < hi {
			consider(i)
		}
	} else {
		// Visual order can differ from storage order. Check grapheme
		// boundaries across this display row and use the nearest caret.
		var seg segmenter.Segmenter
		if err := seg.InitWithString(row); err != nil {
			return 0, ui.Rect{}, false
		}
		for iter := seg.GraphemeIterator(); iter.Next(); {
			consider(lo + iter.Grapheme().OffsetInBytes)
		}
		if hi > len(text) {
			consider(len(text))
		}
	}
	return start + chosen, caret, true
}
