package doc

import (
	"strconv"
	"strings"
)

// TOCKind is the generated outline at a Markdown table-of-contents marker.
type TOCKind struct {
	DividerKind
	Marker string
}

func IsTOCMarker(line string) bool {
	line = strings.TrimSpace(line)
	return strings.EqualFold(line, "[toc]") || strings.EqualFold(line, "[[_TOC_]]")
}

type Heading struct {
	Block BlockID
	Level uint8
	Text  string
	Slug  string
}

// Headings assigns one anchor per heading, even when a written heading
// already ends in the suffix that would be used for a duplicate.
func (d *Document) Headings(slug func(string) string) []Heading {
	var headings []Heading
	used := make(map[string]bool)
	add := func(id BlockID, level uint8, text string) {
		base := slug(text)
		if base == "" {
			base = "heading"
		}
		name := base
		for n := 1; used[name]; n++ {
			name = base + "-" + strconv.Itoa(n)
		}
		used[name] = true
		headings = append(headings, Heading{Block: id, Level: level, Text: text, Slug: name})
	}
	if !d.Title.IsEmpty() {
		add(0, 1, d.Title.PlainText())
	}
	d.ForEachBlock(func(b *Block) {
		if h, ok := b.Kind.(HeadingKind); ok {
			add(b.ID, min(6, max(1, h.Level)), b.Text.PlainText())
		}
	})
	return headings
}
