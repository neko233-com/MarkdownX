package app

import (
	"unicode/utf8"

	"github.com/neko233/markdownx/internal/doc"
)

// typedSpan is the byte range a buffer edit inserted: everything between the
// common prefix and the common suffix of the old and the new text.
func typedSpan(old, new string) (int, int, bool) {
	if old == new {
		return 0, 0, false
	}
	p := 0
	for p < len(old) && p < len(new) && old[p] == new[p] {
		p++
	}
	for p > 0 && p < len(new) && !utf8.RuneStart(new[p]) {
		p--
	}
	q := 0
	for q < len(old)-p && q < len(new)-p && old[len(old)-1-q] == new[len(new)-1-q] {
		q++
	}
	for q > 0 && !utf8.RuneStart(new[len(new)-q]) {
		q--
	}
	return p, len(new) - q, true
}

// applyRangeMarks rebuilds t so [start,end) carries m, keeping the marks of
// the runs outside it.
func applyRangeMarks(t *doc.RichText, start, end int, m doc.Marks) {
	if start >= end || start < 0 || end > len(t.Text) {
		return
	}
	var out doc.RichText
	for _, sp := range t.Spans() {
		run := t.Text[sp.Start:sp.End]
		if sp.End <= start || sp.Start >= end {
			out.Push(run, sp.Marks)
			continue
		}
		if sp.Start < start {
			k := start - sp.Start
			out.Push(run[:k], sp.Marks)
			run = run[k:]
		}
		if sp.End > end {
			k := end - sp.Start
			out.Push(run[:k], m)
			out.Push(run[k:], sp.Marks)
		} else {
			out.Push(run, m)
		}
	}
	*t = out
}
