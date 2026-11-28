package doc

import (
	"html"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui/markdown/parser"
)

// ImageDestination is a writable byte range of an LF Markdown image target.
// Path is the decoded Markdown target, before URL percent-decoding.
type ImageDestination struct {
	Start, End int
	Path       string
}

// ImageDestinations uses parsed image nodes so code, comments and examples
// containing image syntax are left alone. A reference points at the authored
// definition rather than the image's reference label.
func ImageDestinations(source string) []ImageDestination {
	offset := 0
	var model *Document
	if strings.HasPrefix(source, "---\n") {
		model = ReadSource(source)
		if first := model.Blocks[0]; first != nil {
			if kind, ok := first.Kind.(CodeKind); ok && kind.FrontMatter {
				offset = min(len(source), model.source.parts[first.ID].end+1)
			}
		}
	}
	p := parser.New(parser.Default | parser.ExtDollarMath)
	p.Set(source[offset:])
	var targets []ImageDestination
	references := map[string]string{}
	for i := 1; i < p.Len(); i++ {
		for _, in := range p.Inlines(i) {
			if in.Kind != parser.Image || in.Dest == "" || in.Start < 0 || in.Start >= in.End || in.End > len(source)-offset {
				continue
			}
			startOfImage := offset + in.Start
			raw := source[startOfImage : offset+in.End]
			if start, end, ok := imageDestinationBounds(raw, in.Dest); ok {
				targets = append(targets, ImageDestination{startOfImage + start, startOfImage + end, in.Dest})
			} else if label := imageReferenceLabel(raw); label != "" {
				references[labelKey(label)] = in.Dest
			}
		}
	}
	if len(references) > 0 {
		if model == nil {
			model = ReadSource(source)
		}
		targets = append(targets, referencedImageDestinations(model, source, references)...)
	}
	slices.SortFunc(targets, func(a, b ImageDestination) int { return a.Start - b.Start })
	return targets
}

func imageDestinationBounds(raw, decoded string) (start, end int, ok bool) {
	text, found := strings.CutPrefix(raw, "!")
	if !found {
		return 0, 0, false
	}
	// Walk the candidate closing brackets from right to left. An authored
	// caption may contain brackets within code spans or nested formatting.
	for at := strings.LastIndex(text, "]("); at >= 0; at = strings.LastIndex(text[:at], "](") {
		tail := text[at+2:]
		value, used, ok := inlineDestination(tail)
		if !ok || used != len(tail) || html.UnescapeString(unescapeImage(value)) != decoded {
			continue
		}
		trimmed := strings.TrimLeft(tail, " \t\r\n")
		start := 1 + at + 2 + len(tail) - len(trimmed)
		if strings.HasPrefix(trimmed, "<") {
			start++
		}
		return start, start + len(value), true
	}
	return 0, 0, false
}

func imageReferenceLabel(raw string) string {
	if !strings.HasPrefix(raw, "!") {
		return ""
	}
	if ref, used, ok := referenceAt(raw); ok && used == len(raw) && ref.Image {
		return ref.Label
	}
	if caption, end, ok := bracketLabel(raw[1:]); ok && len(raw) == end+1 {
		return caption
	}
	return ""
}

func referencedImageDestinations(d *Document, source string, refs map[string]string) []ImageDestination {
	text, blocks := d.WorkspaceMarkdown()
	if text != source {
		return nil
	}
	kinds := make(map[BlockID]bool, len(d.Blocks))
	for _, b := range d.Blocks {
		_, kinds[b.ID] = b.Kind.(DefinitionKind)
	}
	var targets []ImageDestination
	for _, block := range blocks {
		if !kinds[block.Block] {
			continue
		}
		raw := source[block.Start:block.End]
		line, _, _ := strings.Cut(raw, "\n")
		label, _, footnote, ok := definitionPrefix(line)
		key := labelKey(label)
		target, used := refs[key]
		if !ok || footnote || !used {
			continue
		}
		if link, ok := parseLinkDefinition(label, raw); ok && link.Dest == target {
			if start, end, ok := LinkDefinitionDestination(raw); ok {
				targets = append(targets, ImageDestination{block.Start + start, block.Start + end, target})
			}
			delete(refs, key)
		}
	}
	return targets
}
