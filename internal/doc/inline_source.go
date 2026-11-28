package doc

import (
	"strings"
	"unicode/utf8"
)

// InlineSource associates visible UTF-8 offsets with the authored markup.
type InlineSource struct {
	Start, End int
	Markup     string
}

func (t *RichText) appendMarkup(raw string, value RichText) {
	start := t.Len()
	value.inlineSources = nil
	t.Append(value)
	if start < t.Len() {
		t.inlineSources = append(t.inlineSources, InlineSource{start, t.Len(), raw})
	}
}

// ParseInlineSource applies the workspace's inline grammar and keeps the
// written spelling, including temporarily incomplete syntax and link titles.
func ParseInlineSource(raw string) RichText {
	var out RichText
	out.appendMarkup(raw, mdInlineWith(raw, true, true))
	return out
}

// InlineSourceAt finds a rendered inline at the caret. Atomic objects and
// links can also be picked at a caret edge by a pointer hit in the span.
func (t *RichText) InlineSourceAt(offset int, includeEdge bool) (InlineSource, bool) {
	at := func(lo, hi int) bool { return lo < offset && offset < hi || includeEdge && lo <= offset && offset < hi }
	for _, src := range t.inlineSources {
		if at(src.Start, src.End) && t.Text[src.Start:src.End] != src.Markup {
			return src, true
		}
	}
	for _, span := range t.Spans() {
		if !at(span.Start, span.End) {
			continue
		}
		if in := span.Marks.Inline; in != nil && !in.hasImage() && !in.hasEquation() {
			if _, ref := in.Reference(); !ref {
				break
			}
		}
		value := t.Slice(span.Start, span.End)
		if markup := MarkdownInline(value); markup != "" && markup != value.Text {
			m := span.Marks
			if m.Bold || m.Italic || m.Strike || m.Underline || m.Code || m.Link != nil || m.Inline != nil {
				return InlineSource{span.Start, span.End, markup}, true
			}
		}
		break
	}
	return InlineSource{}, false
}

// PreserveInlineSources restores Markdown spellings lost by a native text
// buffer. Only unchanged text and marks can keep their original source.
func (t *RichText) PreserveInlineSources(previous RichText) {
	if len(previous.inlineSources) == 0 || len(t.inlineSources) != 0 {
		return
	}
	old, newText := previous.Text, t.Text
	start, oldEnd, newEnd := InlineEditBounds(old, newText)
	for _, src := range previous.inlineSources {
		from, to := src.Start, src.End
		switch {
		case src.End <= start:
		case src.Start >= oldEnd:
			src.Start += newEnd - oldEnd
			src.End += newEnd - oldEnd
		default:
			continue
		}
		oldPart, newPart := previous.Slice(from, to), t.Slice(src.Start, src.End)
		if oldPart.sameInlineContent(newPart) {
			t.inlineSources = append(t.inlineSources, src)
		}
	}
}

func (t RichText) sameInlineContent(other RichText) bool {
	if t.Text != other.Text || len(t.Runs) != len(other.Runs) {
		return false
	}
	for i, run := range t.Runs {
		if run.Len != other.Runs[i].Len || !MarksEqual(&run.Marks, &other.Runs[i].Marks) {
			return false
		}
	}
	return true
}

// InlineEditBounds finds the changed interval in two native text snapshots.
func InlineEditBounds(old, next string) (start, oldEnd, newEnd int) {
	for start < min(len(old), len(next)) && old[start] == next[start] {
		start++
	}
	start = floorBoundary(old, start)
	oldEnd, newEnd = len(old), len(next)
	for oldEnd > start && newEnd > start && old[oldEnd-1] == next[newEnd-1] {
		oldEnd--
		newEnd--
	}
	for oldEnd < len(old) && !utf8.RuneStart(old[oldEnd]) {
		oldEnd++
		newEnd++
	}
	return
}

// InlineSourceOffsets maps visible rune boundaries into a markup string.
// The exact label is preferred. Escaped and nested labels can be matched
// rune by rune. An inline object has a caret stop on each side.
func InlineSourceOffsets(raw, visible string) []int {
	if visible == Obj {
		return []int{0, len(raw)}
	}
	if content := strings.Index(raw, visible); content >= 0 && visible != "" {
		offsets := make([]int, 0, utf8.RuneCountInString(visible)+1)
		for at := range visible {
			offsets = append(offsets, content+at)
		}
		return append(offsets, content+len(visible))
	}
	var offsets []int
	at := 0
	for _, r := range visible {
		found := strings.IndexRune(raw[at:], r)
		if found >= 0 {
			at += found
		}
		offsets = append(offsets, at)
		if found >= 0 {
			at += utf8.RuneLen(r)
		}
	}
	return append(offsets, at)
}
