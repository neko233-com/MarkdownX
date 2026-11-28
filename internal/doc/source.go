package doc

import "strings"

// Source keeps the original spelling and whitespace of unchanged workspace
// blocks. The document remains the authority for edited blocks.
type Source struct {
	text  string
	parts map[BlockID]sourcePart
	first BlockID
	last  BlockID
}

type sourcePart struct {
	start, end int
	previous   BlockID
	canonical  string
	separator  string
}

type SourceRange struct {
	Block      BlockID
	Start, End int
}

// ReadSource associates the blocks with their byte ranges in LF Markdown.
func ReadSource(text string) *Document {
	s := &Source{text: text, parts: make(map[BlockID]sourcePart)}
	lines := strings.Split(text, "\n")
	offsets := make([]int, len(lines)+1)
	for i, line := range lines {
		offsets[i+1] = min(len(text), offsets[i]+len(line)+1)
	}
	blocks := mdBlocksLocated(lines, Off{File: true}, func(b *Block, first, end int) {
		// The next block's leading separator includes the last line break.
		lastLineEnd := offsets[end-1] + len(lines[end-1])
		s.parts[b.ID] = sourcePart{start: offsets[first], end: lastLineEnd, previous: s.last}
		if s.first == 0 {
			s.first = b.ID
		}
		s.last = b.ID
	})
	w := &writer{opts: Options{KeepPaths: true}}
	w.eachBlock(blocks, func(b *Block, text, separator string) {
		part := s.parts[b.ID]
		part.canonical, part.separator = text, separator
		s.parts[b.ID] = part
	})
	if len(blocks) == 0 {
		blocks = []*Block{TextBlock("")}
	}
	return &Document{Blocks: blocks, source: s}
}

// WorkspaceMarkdown writes the current document and reports the byte ranges
// of its top-level blocks for navigation between the editor and source view.
func (d *Document) WorkspaceMarkdown() (string, []SourceRange) {
	s := d.source
	w := &writer{opts: Options{KeepPaths: true}}
	var out strings.Builder
	var ranges []SourceRange
	if !d.Title.IsEmpty() {
		out.WriteString("# " + w.rich(&d.Title))
	}
	var previous BlockID
	w.eachBlock(d.Blocks, func(b *Block, text, separator string) {
		if s != nil {
			if part, ok := s.parts[b.ID]; ok {
				if text == part.canonical {
					text = s.text[part.start:part.end]
				}
				switch {
				case previous == 0 && b.ID == s.first && out.Len() == 0:
					out.WriteString(s.text[:part.start])
					separator = ""
				case previous != 0 && previous == part.previous && separator == part.separator:
					separator = s.text[s.parts[previous].end:part.start]
				}
			}
		}
		if out.Len() > 0 {
			out.WriteString(separator)
		}
		start := out.Len()
		out.WriteString(text)
		ranges = append(ranges, SourceRange{Block: b.ID, Start: start, End: out.Len()})
		previous = b.ID
	})
	switch {
	case s != nil && previous != 0 && previous == s.last:
		out.WriteString(s.text[s.parts[previous].end:])
	case s != nil && s.first == 0 && out.Len() == 0:
		out.WriteString(s.text)
	case out.Len() > 0:
		out.WriteByte('\n')
	}
	return out.String(), ranges
}
