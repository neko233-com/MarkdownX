package pdf

import (
	"strconv"
	"strings"
	"unicode"

	"github.com/go-text/typesetting/segmenter"
	"github.com/neko233/markdownx/internal/doc"
)

type color struct{ r, g, b uint8 }

var (
	bodyInk  = color{26, 31, 37}
	mutedInk = color{85, 94, 103}
	linkInk  = color{31, 81, 142}
	ruleInk  = color{206, 211, 215}
	fillInk  = color{245, 247, 249}
)

type style struct {
	weight            weight
	size, rise        float64
	ink               color
	underline, strike bool
	link              string
}

type span struct {
	text    string
	style   style
	picture *picture
}

type piece struct {
	span
	face          *face
	width, height float64
}

type line struct {
	pieces        []piece
	width, height float64
}

type character struct {
	pieces        []piece
	width, height float64
	breakAfter    bool
	space, hard   bool
}

func bodyStyle() style { return style{size: 11, ink: bodyInk} }

func (s style) marked(m doc.Marks) style {
	isBold := m.Bold || s.weight == bold || s.weight == boldItalic
	isItalic := m.Italic || s.weight == italic || s.weight == boldItalic
	switch {
	case m.Code || s.weight == mono:
		s.weight = mono
	case isBold && isItalic:
		s.weight = boldItalic
	case isBold:
		s.weight = bold
	case isItalic:
		s.weight = italic
	}
	s.underline, s.strike = s.underline || m.Underline, s.strike || m.Strike
	if m.TextColor != nil {
		if value := strings.TrimPrefix(*m.TextColor, "#"); len(value) == 6 {
			if n, err := strconv.ParseUint(value, 16, 24); err == nil {
				s.ink = color{uint8(n >> 16), uint8(n >> 8), uint8(n)}
			}
		}
	}
	if m.Link != nil && s.link == "" {
		s.link = *m.Link
	}
	if s.link != "" {
		s.ink, s.underline = linkInk, true
	}
	return s
}

func (w *writer) spans(t doc.RichText, base style) []span {
	var out []span
	for _, r := range t.Spans() {
		s := base.marked(r.Marks)
		if r.Marks.Inline == nil {
			out = append(out, span{text: t.Text[r.Start:r.End], style: s})
			continue
		}
		for range t.Text[r.Start:r.End] {
			out = append(out, w.inlineObject(r.Marks.Inline, s)...)
		}
	}
	return out
}

func (w *writer) inlineObject(in *doc.Inline, s style) []span {
	text := func(value string) []span { return []span{{text: value, style: s}} }
	image := func(name string, p *picture) []span {
		if p != nil {
			return []span{{text: doc.Obj, style: s, picture: p}}
		}
		return text(name)
	}
	if src, ok := in.Equation(); ok {
		return image("$"+src+"$", w.media("math", src, in.IsDisplayEquation()))
	}
	if img, ok := in.Image(); ok {
		return image(img.Alt, w.media("image", img.Src, false))
	}
	if ref, ok := in.Reference(); ok {
		if ref.Footnote {
			if note, found := w.refs.Footnote(ref.Label); found && note.Number > 0 {
				s.rise, s.size = s.size*0.33, s.size*0.72
				s.link, s.ink, s.underline = noteTarget(note.Number), linkInk, false
				return text(strconv.Itoa(note.Number))
			}
		} else if link, found := w.refs.Link(ref.Label); found {
			if ref.Image {
				return image(ref.PlainText(), w.media("image", link.Destination, false))
			}
			s.link, s.ink, s.underline = link.Destination, linkInk, true
			// Reference captions may carry inline formatting as ordinary links do.
			return w.spans(doc.ParseInlineSource(ref.Caption), s)
		}
		return text(ref.Markup)
	}
	if file, ok := in.File(); ok {
		return text(file.Name)
	}
	if link, ok := in.DocLink(); ok {
		return text(link.Title)
	}
	if button, ok := in.Button(); ok {
		return text(button.Text)
	}
	return nil
}

func (w *writer) characters(spans []span, width float64) []character {
	var input strings.Builder
	ends := make([]int, len(spans))
	for i, s := range spans {
		s.text = strings.ReplaceAll(s.text, "\t", "    ")
		spans[i] = s
		input.WriteString(s.text)
		ends[i] = input.Len()
	}
	content := input.String()
	var segmenter segmenter.Segmenter
	if err := segmenter.InitWithString(content); err != nil {
		w.fail(err)
		return nil
	}
	breaks := make(map[int]bool)
	for iter := segmenter.LineIterator(); iter.Next(); {
		l := iter.Line()
		breaks[l.OffsetInBytes+l.LengthInBytes] = true
	}
	var out []character
	var i int
	for iter := segmenter.GraphemeIterator(); iter.Next(); {
		g := iter.Grapheme()
		from, to := g.OffsetInBytes, g.OffsetInBytes+g.LengthInBytes
		cluster := content[from:to]
		newline := strings.ContainsAny(cluster, "\r\n\u2028\u2029")
		c := character{space: strings.TrimSpace(cluster) == "", hard: newline, breakAfter: breaks[to]}
		for from < to && !newline {
			for i < len(ends) && from >= ends[i] {
				i++
			}
			if i == len(ends) {
				break
			}
			end := min(ends[i], to)
			s := spans[i]
			s.text = content[from:end]
			w.measure(&c, s, width)
			from = end
		}
		out = append(out, c)
	}
	return out
}

func (w *writer) measure(c *character, s span, width float64) {
	if s.picture != nil {
		pw, ph := s.picture.fit(width, 56)
		c.append(piece{span: s, width: pw, height: ph + 2})
		return
	}
	for _, r := range s.text {
		if r == '\u00ad' || r == '\ufeff' || unicode.Is(unicode.Cf, r) || r == '\ufe0e' || r == '\ufe0f' {
			continue
		}
		font, advance, err := w.fonts.selectGlyph(s.style.weight, r, s.style.size)
		if err != nil {
			w.fail(err)
			return
		}
		glyph := s
		glyph.text = string(r)
		c.append(piece{span: glyph, face: font, width: advance, height: (s.style.size + s.style.rise) * 1.48})
	}
}

func (c *character) append(p piece) {
	c.width += p.width
	c.height = max(c.height, p.height)
	c.pieces = append(c.pieces, p)
}

func (w *writer) wrap(spans []span, base style, width float64, literal bool) []line {
	defaultHeight := base.size * 1.48
	chars := w.characters(spans, width)
	if len(chars) == 0 {
		return []line{{height: defaultHeight}}
	}
	var lines []line
	for start := 0; start < len(chars); {
		end, permitted, advance := start, start, 0.0
		for end < len(chars) && !chars[end].hard {
			if end > start && advance+chars[end].width > width {
				if permitted > start {
					end = permitted
				}
				break
			}
			advance += chars[end].width
			if chars[end].breakAfter {
				permitted = end + 1
			}
			end++
		}
		last := end
		if !literal {
			for last > start && chars[last-1].space {
				last--
			}
		}
		l := line{height: defaultHeight}
		for _, c := range chars[start:last] {
			l.width += c.width
			l.height = max(l.height, c.height)
			for _, p := range c.pieces {
				l.append(p)
			}
		}
		lines = append(lines, l)
		start = end
		if !literal {
			for start < len(chars) && chars[start].space && !chars[start].hard {
				start++
			}
		}
		if start < len(chars) && chars[start].hard {
			start++
		}
	}
	return lines
}

func (l *line) append(p piece) {
	if i := len(l.pieces) - 1; i >= 0 && p.picture == nil {
		previous := &l.pieces[i]
		if previous.picture == nil && previous.style == p.style && previous.face == p.face {
			previous.text += p.text
			previous.width += p.width
			return
		}
	}
	l.pieces = append(l.pieces, p)
}

func (w *writer) drawLine(l line, x, y float64) {
	for _, p := range l.pieces {
		if p.picture != nil {
			w.drawPicture(p.picture, x, y+l.height-p.height, p.width, p.height-2)
			x += p.width
			continue
		}
		s := p.style
		baseline := y + l.height - max(3, l.height*0.22) - s.rise
		w.fail(w.pdf.SetFont(p.face.name, "", s.size))
		w.pdf.SetTextColor(s.ink.r, s.ink.g, s.ink.b)
		w.pdf.SetXY(x, baseline)
		w.fail(w.pdf.Text(p.text))
		if s.underline || s.strike {
			w.pdf.SetStrokeColor(s.ink.r, s.ink.g, s.ink.b)
			w.pdf.SetLineWidth(0.6)
			if s.underline {
				w.pdf.Line(x, baseline+1.6, x+p.width, baseline+1.6)
			}
			if s.strike {
				w.pdf.Line(x, baseline-s.size*0.28, x+p.width, baseline-s.size*0.28)
			}
		}
		if s.link != "" {
			w.addLink(s.link, x, baseline-s.size, p.width, s.size+3)
		}
		x += p.width
	}
}
