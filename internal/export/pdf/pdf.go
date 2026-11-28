// Package pdf lays out a MarkdownX document on paper and writes a PDF with
// selectable text, heading bookmarks, and links between pages.
package pdf

import (
	"bytes"
	"fmt"
	"image"
	"image/png"
	"io"
	"math"
	"net/url"
	"strconv"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui/markdown"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/signintech/gopdf"
)

type Options struct {
	Paper, Title, Header, Footer string
	MarginMM                     float64
	Landscape, BreakAtH1         bool
	// Media resolves local images or rasterizes equations and diagrams.
	// Unknown media returns nil so the original source or alt text can print.
	Media func(kind, source string, display bool) image.Image
}

func Defaults() Options {
	return Options{Paper: "A4", MarginMM: 18, Footer: "${pageNo} / ${pageCount}"}
}

const pointsPerMM = 72.0 / 25.4

type writer struct {
	pdf             gopdf.GoPdf
	fonts           fontSet
	opts            Options
	refs            doc.References
	headings        []doc.Heading
	byBlock         map[doc.BlockID]doc.Heading
	anchors         map[string]bool
	noteReturns     map[string]string
	mediaCache      map[mediaKey]*picture
	w, h, margin, y float64
	h1Seen          bool
	chinese         bool
	err             error
}

func Write(out io.Writer, d *doc.Document, opts Options) error {
	width, height, margin, err := pageGeometry(opts)
	if err != nil {
		return err
	}
	w := &writer{
		opts: opts, w: width, h: height, margin: margin,
		chinese: strings.ContainsFunc(d.PlainText(), func(r rune) bool { return unicode.Is(unicode.Han, r) }),
		refs:    d.References(), headings: d.Headings(markdown.Slug),
		byBlock: make(map[doc.BlockID]doc.Heading), anchors: make(map[string]bool),
		noteReturns: make(map[string]string), mediaCache: make(map[mediaKey]*picture),
	}
	w.fonts = fontSet{pdf: &w.pdf, faces: make(map[weight][]*face), files: make(map[string]*face), missing: make(map[weight]int)}
	w.pdf.Start(gopdf.Config{PageSize: gopdf.Rect{W: width, H: height}})
	w.pdf.SetInfo(gopdf.PdfInfo{Title: opts.Title, Creator: "MarkdownX"})
	for _, heading := range w.headings {
		w.byBlock[heading.Block] = heading
	}
	w.page()
	// Register destinations before drawing links. The writer resolves their
	// final positions later, and annotations stay on the page that draws them.
	for _, heading := range w.headings {
		w.anchor("#"+heading.Slug, w.margin)
	}
	for _, note := range w.refs.Footnotes() {
		w.anchor(noteTarget(note.Number), w.margin)
	}
	if !d.Title.IsEmpty() {
		w.heading(w.byBlock[0], d.Title, margin, w.bodyWidth(), false)
	}
	w.blocks(d.Blocks, margin, w.bodyWidth(), false)
	w.footnotes()
	w.finishPages()
	if w.err != nil {
		return w.err
	}
	_, err = w.pdf.WriteTo(out)
	return err
}

func pageGeometry(opts Options) (width, height, margin float64, err error) {
	switch opts.Paper {
	case "", "A4":
		width, height = 210*pointsPerMM, 297*pointsPerMM
	case "A5":
		width, height = 148*pointsPerMM, 210*pointsPerMM
	case "Letter":
		width, height = 612, 792
	case "Legal":
		width, height = 612, 1008
	default:
		return 0, 0, 0, fmt.Errorf("不支持的纸张尺寸 %q", opts.Paper)
	}
	if opts.Landscape {
		width, height = height, width
	}
	if opts.MarginMM == 0 {
		opts.MarginMM = Defaults().MarginMM
	}
	margin = opts.MarginMM * pointsPerMM
	if math.IsNaN(margin) || margin < 5*pointsPerMM || 2*margin >= min(width, height)-144 {
		return 0, 0, 0, fmt.Errorf("页边距超出纸张范围")
	}
	return width, height, margin, nil
}

func (w *writer) fail(err error) {
	if w.err == nil && err != nil {
		w.err = err
	}
}

func (w *writer) page() {
	w.pdf.AddPage()
	w.y = w.margin
}

func (w *writer) bodyWidth() float64  { return w.w - 2*w.margin }
func (w *writer) bodyHeight() float64 { return w.h - 2*w.margin }
func (w *writer) available() float64  { return w.h - w.margin - w.y }

func (w *writer) keep(height float64) {
	if w.y > w.margin && height > w.available() {
		w.page()
	}
}

func (w *writer) newSection() {
	if w.y > w.margin {
		w.page()
	}
}

func (w *writer) blocks(blocks []*doc.Block, x, width float64, quote bool) {
	seq := uint32(0)
	for _, b := range blocks {
		if w.err != nil {
			return
		}
		s := bodyStyle()
		marker := ""
		if k, ok := b.Kind.(doc.OrderedKind); ok {
			seq++
			if k.Seq != nil {
				seq = *k.Seq
			}
			marker = strconv.FormatUint(uint64(seq), 10) + ". "
		} else {
			seq = 0
		}
		switch k := b.Kind.(type) {
		case doc.TextKind:
			if isPageBreak(b.Text.Text) {
				w.newSection()
				continue
			}
			if eq, ok := displayMath(b.Text); ok {
				if p := w.media("math", eq, true); p != nil {
					w.figure(p, x, width, doc.AlignCenter)
					continue
				}
			}
			w.paragraph(w.spans(b.Text, s), s, x, width, b.Align, false, quote, 7)
			w.children(b, x, width, quote)
		case doc.HeadingKind:
			w.heading(w.byBlock[b.ID], b.Text, x, width, quote)
			w.children(b, x, width, quote)
		case doc.BulletKind, doc.OrderedKind, doc.TodoKind:
			switch t := b.Kind.(type) {
			case doc.BulletKind:
				marker = "• "
			case doc.TodoKind:
				marker = "[ ] "
				if t.Done {
					marker = "[x] "
				}
			}
			spans := append([]span{{text: marker, style: s}}, w.spans(b.Text, s)...)
			w.paragraph(spans, s, x, width, b.Align, false, quote, 4)
			w.children(b, x, width, quote)
		case doc.CodeKind:
			if k.FrontMatter {
				continue
			}
			if p := w.media(k.Lang, b.Text.Text, true); p != nil && k.Lang != "" {
				w.figure(p, x, width, doc.AlignCenter)
				continue
			}
			s.size, s.weight = 9.5, mono
			w.paragraph([]span{{text: b.Text.Text, style: s}}, s, x, width, doc.AlignLeft, true, quote, 9)
		case doc.QuoteKind, doc.CalloutKind:
			inset := indent(width)
			w.blocks(b.Children, x+inset, width-inset, true)
		case doc.DividerKind:
			w.keep(22)
			w.rule(x, w.y+9, x+width, w.y+9)
			w.y += 22
		case doc.TOCKind:
			w.contents(x, width)
		case doc.DefinitionKind:
			if doc.DefinitionBlock(b.Text.Text) == nil {
				w.paragraph(w.spans(b.Text, s), s, x, width, b.Align, false, quote, 7)
			}
		case doc.ImageKind:
			if p := w.media("image", k.Src, false); p != nil {
				w.figure(p, x, width, b.Align)
			} else if k.Caption != nil {
				w.paragraph([]span{{text: *k.Caption, style: s}}, s, x, width, b.Align, false, quote, 9)
			}
		case doc.TableKind:
			w.table(k.Table, x, width)
		case doc.FileKind:
			w.paragraph([]span{{text: k.File.Name, style: s}}, s, x, width, b.Align, false, quote, 7)
		case doc.GridKind, doc.ColumnKind:
			w.blocks(b.Children, x, width, quote)
		}
	}
}

func indent(width float64) float64 { return min(16, max(0, width-80)) }

func (w *writer) children(b *doc.Block, x, width float64, quote bool) {
	offset := indent(width)
	w.blocks(b.Children, x+offset, width-offset, quote)
}

func isPageBreak(text string) bool {
	compact := strings.Map(func(r rune) rune {
		if unicode.IsSpace(r) {
			return -1
		}
		if r == '\'' {
			return '"'
		}
		return unicode.ToLower(r)
	}, text)
	switch compact {
	case "<!--pagebreak-->", `<divstyle="page-break-after:always"></div>`, `<divstyle="page-break-after:always;"></div>`,
		`<divstyle="break-after:page"></div>`, `<divstyle="break-after:page;"></div>`:
		return true
	}
	return false
}

func displayMath(t doc.RichText) (string, bool) {
	for _, run := range t.Runs {
		if run.Marks.Inline.IsDisplayEquation() && t.Text == doc.Obj {
			return run.Marks.Inline.Equation()
		}
	}
	return "", false
}

func (w *writer) heading(h doc.Heading, t doc.RichText, x, width float64, quote bool) {
	s := bodyStyle()
	s.size, s.weight = max(12.0, 23-2*float64(h.Level)), bold
	lines := w.wrap(w.spans(t, s), s, width, false)
	if h.Level == 1 && !quote {
		if w.opts.BreakAtH1 && w.h1Seen {
			w.newSection()
		}
		w.h1Seen = true
	}
	var firstHeight float64
	for _, l := range lines[:min(2, len(lines))] {
		firstHeight += l.height
	}
	w.keep(firstHeight + 26)
	if w.y > w.margin {
		w.y += 5
	}
	if h.Slug != "" {
		w.anchor("#"+h.Slug, w.y)
		w.pdf.AddOutlineWithPosition(firstNonEmpty(h.Text, "Heading"))
	}
	w.drawParagraph(lines, x, width, doc.AlignLeft, false, quote)
	w.y += 7
}

func (w *writer) paragraph(s []span, base style, x, width float64, align doc.Align, literal, quote bool, gap float64) {
	textX, textWidth := x, width
	if literal {
		textX, textWidth = x+7, width-14
	}
	lines := w.wrap(s, base, textWidth, literal)
	w.drawParagraph(lines, textX, textWidth, align, literal, quote)
	w.y += gap
}

func (w *writer) drawParagraph(lines []line, x, width float64, align doc.Align, literal, quote bool) {
	for _, l := range lines {
		w.keep(l.height)
		if literal {
			w.fill(x-7, w.y, width+14, l.height, fillInk)
		}
		if quote {
			w.rule(x-7, w.y, x-7, w.y+l.height)
		}
		left := x
		if align == doc.AlignCenter {
			left += max(0, width-l.width) / 2
		} else if align == doc.AlignRight {
			left += max(0, width-l.width)
		}
		w.drawLine(l, left, w.y)
		w.y += l.height
	}
}

func (w *writer) contents(x, width float64) {
	s := bodyStyle()
	s.size, s.ink = 10.5, linkInk
	minimum := uint8(6)
	for _, h := range w.headings {
		minimum = min(minimum, h.Level)
	}
	for _, h := range w.headings {
		dx := float64(h.Level-minimum) * 14
		s.link = "#" + h.Slug
		w.paragraph([]span{{text: firstNonEmpty(h.Text, "Heading"), style: s}}, s, x+dx, width-dx, doc.AlignLeft, false, false, 1)
	}
	w.y += 8
}

func noteTarget(n int) string { return "\x00note" + strconv.Itoa(n) }

func (w *writer) footnotes() {
	notes := w.refs.Footnotes()
	if len(notes) == 0 {
		return
	}
	if w.opts.BreakAtH1 {
		w.newSection()
	}
	s := bodyStyle()
	s.weight, s.size = bold, 13
	w.keep(60)
	w.rule(w.margin, w.y+5, w.margin+w.bodyWidth(), w.y+5)
	w.y += 12
	title, backlink := "Footnotes", "Back to reference"
	if w.chinese {
		title, backlink = "脚注", "返回引用"
	}
	w.paragraph([]span{{text: title, style: s}}, s, w.margin, w.bodyWidth(), doc.AlignLeft, false, false, 6)
	for _, note := range notes {
		target := noteTarget(note.Number)
		w.keep(40)
		w.anchor(target, w.y)
		number := span{text: strconv.Itoa(note.Number) + ".", style: s}
		var label = []span{number}
		if link := w.noteReturns[target]; link != "" {
			back := style{size: 9, ink: linkInk, link: link}
			label = append(label, span{text: "  " + backlink, style: back})
		}
		w.paragraph(label, s, w.margin, w.bodyWidth(), doc.AlignLeft, false, false, 0)
		w.blocks(note.Body, w.margin+14, w.bodyWidth()-14, false)
	}
}

func (w *writer) rule(x1, y1, x2, y2 float64) {
	w.pdf.SetStrokeColor(ruleInk.r, ruleInk.g, ruleInk.b)
	w.pdf.SetLineWidth(0.6)
	w.pdf.Line(x1, y1, x2, y2)
}

func (w *writer) fill(x, y, width, height float64, c color) {
	w.pdf.SetFillColor(c.r, c.g, c.b)
	w.pdf.RectFromUpperLeftWithStyle(x, y, width, height, "F")
}

func (w *writer) anchor(name string, y float64) {
	w.pdf.SetY(y + 10)
	w.pdf.SetAnchor(name)
	w.anchors[name] = true
}

func (w *writer) addLink(target string, x, y, width, height float64) {
	if width <= 0 {
		return
	}
	if target == "" {
		return
	}
	if target[0] == '#' || target[0] == 0 {
		if target[0] == '#' {
			if decoded, err := url.PathUnescape(target); err == nil {
				target = strings.ToLower(decoded)
			}
		}
		if strings.HasPrefix(target, "\x00note") && w.noteReturns[target] == "" {
			back := "\x00return" + target[1:]
			w.anchor(back, y)
			w.noteReturns[target] = back
		}
		if w.anchors[target] {
			w.pdf.AddInternalLink(target, x, y, width, height)
		}
		return
	}
	if strings.HasPrefix(strings.ToLower(target), "www.") {
		target = "https://" + target
	}
	u, err := url.Parse(target)
	if err != nil || strings.ContainsFunc(target, unicode.IsControl) {
		return
	}
	switch strings.ToLower(u.Scheme) {
	case "http", "https", "mailto":
		w.pdf.AddExternalLink(u.String(), x, y, width, height)
	}
}

func (w *writer) finishPages() {
	pages := w.pdf.GetNumberOfPages()
	for page := 1; page <= pages; page++ {
		if w.opts.Header == "" && w.opts.Footer == "" {
			return
		}
		w.fail(w.pdf.SetPage(page))
		expand := strings.NewReplacer("${title}", w.opts.Title, "${pageNo}", strconv.Itoa(page), "${pageCount}", strconv.Itoa(pages))
		w.pageLabel(expand.Replace(w.opts.Header), w.margin*0.36)
		w.pageLabel(expand.Replace(w.opts.Footer), w.h-w.margin*0.63)
	}
}

func (w *writer) pageLabel(text string, y float64) {
	if text == "" {
		return
	}
	s := style{size: 8.5, ink: mutedInk}
	wrapped := w.wrap([]span{{text: text, style: s}}, s, w.bodyWidth(), false)
	l := wrapped[0]
	w.drawLine(l, w.margin+(w.bodyWidth()-l.width)/2, y)
}

type mediaKey struct {
	kind, source string
	display      bool
}

type picture struct {
	holder gopdf.ImageHolder
	w, h   float64
}

func (w *writer) media(kind, source string, display bool) *picture {
	if w.opts.Media == nil || source == "" || kind == "" {
		return nil
	}
	k := mediaKey{kind, source, display}
	if p, found := w.mediaCache[k]; found {
		return p
	}
	w.mediaCache[k] = nil
	i := w.opts.Media(kind, source, display)
	if i == nil || i.Bounds().Empty() {
		return nil
	}
	var pngData bytes.Buffer
	if err := png.Encode(&pngData, i); err != nil {
		return nil
	}
	holder, err := gopdf.ImageHolderByBytes(pngData.Bytes())
	if err != nil {
		return nil
	}
	scale := 0.75
	if kind == "math" {
		scale = 0.5
	}
	p := &picture{holder, float64(i.Bounds().Dx()) * scale, float64(i.Bounds().Dy()) * scale}
	w.mediaCache[k] = p
	return p
}

func (p *picture) fit(maxWidth, maxHeight float64) (float64, float64) {
	scale := min(1, maxWidth/p.w, maxHeight/p.h)
	return p.w * scale, p.h * scale
}

func (w *writer) figure(p *picture, x, width float64, align doc.Align) {
	pw, ph := p.fit(width, w.bodyHeight()-10)
	w.keep(ph + 9)
	if align == doc.AlignCenter {
		x += (width - pw) / 2
	} else if align == doc.AlignRight {
		x += width - pw
	}
	w.drawPicture(p, x, w.y, pw, ph)
	w.y += ph + 9
}

func (w *writer) drawPicture(p *picture, x, y, width, height float64) {
	w.fail(w.pdf.ImageByHolder(p.holder, x, y, &gopdf.Rect{W: width, H: height}))
}

func firstNonEmpty(a, b string) string {
	if a != "" {
		return a
	}
	return b
}
