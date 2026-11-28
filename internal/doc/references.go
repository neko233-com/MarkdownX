package doc

import (
	"slices"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui/markdown/parser"
)

// DefinitionKind holds the editable source of a link or footnote definition.
type DefinitionKind struct{ TextKind }

// ReferenceInline retains the spelling of a reference even when its target
// does not exist in the current document or is edited later.
type ReferenceInline struct {
	Label, Caption, Markup string
	Footnote               bool
	Image                  bool
}

func (ReferenceInline) inlineKind() {}

func (in *Inline) Reference() (ReferenceInline, bool) {
	if in == nil {
		return ReferenceInline{}, false
	}
	r, ok := in.Kind.(ReferenceInline)
	return r, ok
}

// ReferenceObject restores the same object from a portable book's XML.
func ReferenceObject(source string) (Inline, bool) {
	ref, used, ok := referenceAt(source)
	if !ok || used != len(source) {
		return Inline{}, false
	}
	return referenceObject(ref), true
}

func referenceObject(ref ReferenceInline) Inline {
	return Inline{Kind: ref, XML: `<markdown-reference source="` + escapeReferenceAttribute.Replace(ref.Markup) + `"/>`}
}

var escapeReferenceAttribute = strings.NewReplacer("&", "&amp;", `"`, "&quot;", "<", "&lt;", ">", "&gt;", "\t", "&#9;")

// bracketLabel scans one bracketed label, including labels with nested
// brackets. Escaped punctuation cannot close a bracket.
func bracketLabel(s string) (label string, end int, ok bool) {
	if !strings.HasPrefix(s, "[") {
		return "", 0, false
	}
	depth := 1
	for i := 1; i < len(s) && i <= 1000; i++ {
		switch s[i] {
		case '\\':
			if i+1 < len(s) && isASCIIPunct(s[i+1]) {
				i++
			}
		case '[':
			depth++
		case ']':
			depth--
			if depth == 0 {
				return s[1:i], i + 1, true
			}
		case '\n', '\r':
			return "", 0, false
		}
	}
	return "", 0, false
}

func referenceAt(s string) (ReferenceInline, int, bool) {
	text, image := strings.CutPrefix(s, "!")
	label, end, ok := bracketLabel(text)
	if !ok || label == "" {
		return ReferenceInline{}, 0, false
	}
	if strings.HasPrefix(label, "^") {
		if !image && validFootnoteLabel(label[1:]) && !strings.HasPrefix(text[end:], ":") {
			return ReferenceInline{Label: label[1:], Markup: s[:end], Footnote: true}, end, true
		}
		return ReferenceInline{}, 0, false
	}
	target, n, ok := bracketLabel(text[end:])
	if !ok {
		return ReferenceInline{}, 0, false
	}
	if target == "" {
		target = label
	}
	if labelKey(target) == "" {
		return ReferenceInline{}, 0, false
	}
	end += n
	if image {
		end++
	}
	return ReferenceInline{Label: target, Caption: label, Markup: s[:end], Image: image}, end, true
}

func validFootnoteLabel(s string) bool {
	return s != "" && !strings.ContainsAny(s, "[]\x00") && !strings.ContainsFunc(s, unicode.IsSpace)
}

func labelKey(s string) string { return strings.ToLower(strings.Join(strings.Fields(s), " ")) }

func definitionPrefix(s string) (label, rest string, footnote, ok bool) {
	label, end, found := bracketLabel(strings.TrimLeft(s, " \t"))
	if !found || labelKey(label) == "" || IndentOf(s) > 3 {
		return "", "", false, false
	}
	rest, ok = strings.CutPrefix(strings.TrimLeft(s, " \t")[end:], ":")
	if !ok {
		return "", "", false, false
	}
	if name, isNote := strings.CutPrefix(label, "^"); isNote {
		return name, rest, true, validFootnoteLabel(name)
	}
	return label, rest, false, true
}

// DefinitionDraft keeps a line with a definition prefix editable as source.
func DefinitionDraft(source string) bool {
	if _, _, _, ok := definitionPrefix(source); ok {
		return true
	}
	s := strings.TrimSpace(source)
	label, end, ok := bracketLabel(s)
	return ok && end == len(s) && strings.HasPrefix(label, "^") && validFootnoteLabel(label[1:])
}

// DefinitionBlock validates a single definition typed in the editor.
func DefinitionBlock(source string) *Block {
	lines := strings.Split(source, "\n")
	if definitionLines(lines) != len(lines) {
		return nil
	}
	return NewBlock(DefinitionKind{}, PlainRich(source))
}

func definitionLines(lines []string) int {
	label, rest, footnote, ok := definitionPrefix(lines[0])
	if !ok {
		return 0
	}
	if footnote {
		end := 1
		for i := 1; i < len(lines); i++ {
			if strings.TrimSpace(lines[i]) == "" {
				continue
			}
			if IndentOf(lines[i]) < 4 {
				break
			}
			end = i + 1
		}
		return end
	}
	// A link destination can be on the next line, and an optional title
	// can be on its own line. The MyGo grammar decides which lines belong.
	end := 1
	if strings.TrimSpace(rest) == "" && end < len(lines) && strings.TrimSpace(lines[end]) != "" {
		end++
	}
	if end < len(lines) {
		title := strings.TrimLeft(lines[end], " \t")
		if title != "" && strings.ContainsRune(`"'(`, rune(title[0])) {
			end++
		}
	}
	for ; end > 0; end-- {
		if _, ok := parseLinkDefinition(label, strings.Join(lines[:end], "\n")); ok {
			return end
		}
	}
	return 0
}

func parseLinkDefinition(label, source string) (parser.Inline, bool) {
	written := "[target][" + label + "]\n\n"
	d := parser.Parse(written + source)
	if d.Len() == 2 {
		for _, in := range d.Inlines(1) {
			if in.Kind == parser.Link && in.Start == 0 {
				return in, true
			}
		}
	}
	return parser.Inline{}, false
}

type LinkDefinition struct {
	Block              BlockID
	Destination, Title string
}

// LinkDefinitionDestination locates the authored URL in an editable definition.
func LinkDefinitionDestination(source string) (start, end int, ok bool) {
	first, _, _ := strings.Cut(source, "\n")
	_, rest, note, found := definitionPrefix(first)
	if !found || note {
		return 0, 0, false
	}
	value := strings.TrimLeft(source[len(first)-len(rest):], " \t\r\n")
	start = len(source) - len(value)
	target, _, found := inlineDestination(value + ")")
	if !found {
		return 0, 0, false
	}
	if strings.HasPrefix(value, "<") {
		start++
	}
	return start, start + len(target), true
}

func (r ReferenceInline) PlainText() string {
	if r.Footnote {
		return r.Markup
	}
	t := mdInlineWith(r.Caption, true, false)
	return t.PlainText()
}

type Footnote struct {
	Block        BlockID
	Label        string
	Number       int
	Body         []*Block
	OriginBlock  BlockID
	OriginOffset int
}

type References struct {
	links map[string]LinkDefinition
	notes map[string]*Footnote
	byID  map[BlockID]*Footnote
	order []*Footnote
}

func (r References) Link(label string) (LinkDefinition, bool) {
	l, ok := r.links[labelKey(label)]
	return l, ok
}

func (r References) Footnote(label string) (*Footnote, bool) {
	n, ok := r.notes[labelKey(label)]
	return n, ok
}

func (r References) Footnotes() []*Footnote { return slices.Clone(r.order) }

func (r References) FootnoteDefinition(id BlockID) *Footnote { return r.byID[id] }

func (d *Document) References() References {
	return d.ReferencesFrom(d)
}

// ReferencesFrom resolves the rendered document against definitions from the
// original document, as when copying a selection that omits its definitions.
func (d *Document) ReferencesFrom(source *Document) References {
	r := References{links: make(map[string]LinkDefinition), notes: make(map[string]*Footnote), byID: make(map[BlockID]*Footnote)}
	source.ForEachBlock(func(b *Block) {
		if _, definition := b.Kind.(DefinitionKind); !definition {
			return
		}
		first, more, _ := strings.Cut(b.Text.Text, "\n")
		label, rest, footnote, ok := definitionPrefix(first)
		if !ok {
			return
		}
		key := labelKey(label)
		if footnote {
			if _, exists := r.notes[key]; exists {
				return
			}
			lines := []string{strings.TrimLeft(rest, " \t")}
			if more != "" {
				for _, line := range strings.Split(more, "\n") {
					lines = append(lines, Dedent(line, 4))
				}
			}
			note := &Footnote{Block: b.ID, Label: label, Body: mdBlocks(lines, Off{File: true})}
			r.notes[key], r.byID[b.ID] = note, note
		} else if _, exists := r.links[key]; !exists {
			if l, ok := parseLinkDefinition(label, b.Text.Text); ok {
				r.links[key] = LinkDefinition{Block: b.ID, Destination: l.Dest, Title: l.Title}
			}
		}
	})
	if len(r.notes) == 0 {
		return r
	}
	register := func(id BlockID, t RichText) {
		offset := 0
		for _, run := range t.Runs {
			if ref, ok := run.Marks.Inline.Reference(); ok && ref.Footnote {
				if note, ok := r.Footnote(ref.Label); ok && note.Number == 0 {
					note.Number = len(r.order) + 1
					note.OriginBlock, note.OriginOffset = id, offset
					r.order = append(r.order, note)
				}
			}
			offset += run.Len
		}
	}
	register(0, d.Title)
	d.ForEachBlock(func(b *Block) { register(b.ID, b.Text) })
	for i := 0; i < len(r.order); i++ {
		note := r.order[i]
		(&Document{Blocks: note.Body}).ForEachBlock(func(b *Block) { register(note.Block, b.Text) })
	}
	return r
}
