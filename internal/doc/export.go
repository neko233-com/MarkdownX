package doc

import (
	"encoding/json"
	"fmt"
	"path/filepath"
	"strings"
	"unicode"
	"unicode/utf8"
)

// Options are the 导出 Markdown settings.
type Options struct {
	// KeepPaths writes Markdown references in place, without copying assets.
	KeepPaths bool
	// Resolve turns a library-relative source into an absolute file path.
	// Nil means local assets cannot be resolved and count as missing.
	Resolve func(src string) (string, bool)
	// SubPages are the titles 子页面列表 shows.
	SubPages []string
	// Assets is the folder images are copied into next to the .md file.
	// Nil is 仅文本（不含图片和附件）.
	Assets *string
	// DotRelative prefixes newly written relative links with `./`.
	DotRelative bool
}

// ExportResult is the Markdown text plus the local files to copy.
type ExportResult struct {
	Missing int
	Text    string
	// Files are (absolute source, path relative to the .md file).
	Files [][2]string
}

// ToMarkdown renders a document with the default export settings.
func ToMarkdown(d *Document) string {
	dir := "assets"
	return Export(d, Options{Assets: &dir}).Text
}

// Export renders the whole document as CommonMark/GFM.
func Export(d *Document, opts Options) ExportResult {
	w := &writer{opts: opts}
	var out strings.Builder
	title := strings.TrimSpace(d.Title.Text)
	if title != "" {
		start := len(d.Title.Text) - len(strings.TrimLeftFunc(d.Title.Text, unicode.IsSpace))
		rich := d.Title.Slice(start, start+len(title))
		out.WriteString("# ")
		out.WriteString(w.rich(&rich))
		out.WriteString("\n\n")
	}
	out.WriteString(w.blocks(d.Blocks))
	text := strings.TrimRight(out.String(), " \t\n\r")
	text += "\n"
	return ExportResult{Missing: w.missing, Text: text, Files: w.files}
}

type writer struct {
	missing int
	opts    Options
	files   [][2]string
}

// blocks renders blocks separated by blank lines; consecutive list items stay
// tight.
func (w *writer) blocks(bs []*Block) string {
	var out strings.Builder
	w.eachBlock(bs, func(_ *Block, text, separator string) {
		if out.Len() > 0 {
			out.WriteString(separator)
		}
		out.WriteString(text)
	})
	return out.String()
}

func (w *writer) eachBlock(bs []*Block, emit func(*Block, string, string)) {
	prevList := false
	var seq uint32
	for _, b := range bs {
		list := b.Kind.IsList()
		if s, ok := b.Kind.(OrderedKind); ok {
			if s.Seq != nil {
				seq = *s.Seq
			} else if prevList && seq > 0 {
				seq++
			} else {
				seq = 1
			}
		} else {
			seq = 0
		}
		s := w.block(b, seq)
		if s == "" && !list {
			continue
		}
		separator := "\n\n"
		o, ordered := b.Kind.(OrderedKind)
		if list && prevList && (!ordered || o.Seq == nil) {
			separator = "\n"
		}
		emit(b, s, separator)
		prevList = list
	}
}

func (w *writer) block(b *Block, seq uint32) string {
	var display *string
	if _, ok := b.Kind.(TextKind); ok && len(b.Text.Runs) == 1 && b.Text.Text == Obj {
		if in := b.Text.Runs[0].Marks.Inline; in != nil && in.IsDisplayEquation() {
			if src, ok := in.Equation(); ok {
				display = &src
			}
		}
	}
	var text string
	if display != nil {
		text = "$$\n" + *display + "\n$$"
	} else {
		text = w.rich(&b.Text)
	}
	nested := func(head string) string {
		if len(b.Children) == 0 {
			return head
		}
		var prefix string
		switch b.Kind.(type) {
		case BulletKind, TodoKind:
			prefix = "  "
		case OrderedKind:
			prefix = strings.Repeat(" ", len(fmt.Sprint(seq))+2)
		default:
			prefix = "    "
		}
		return head + "\n" + indent(w.blocks(b.Children), prefix)
	}
	switch k := b.Kind.(type) {
	case TextKind:
		rendered := text
		if display == nil {
			lines := strings.Split(text, "\n")
			for i, line := range lines {
				lines[i] = escapeParagraphStart(line)
			}
			rendered = ReplaceHardBreaksInParagraph(strings.Join(lines, "\n"))
		}
		leading := 0
		for leading < len(rendered) && rendered[leading] == ' ' {
			leading++
		}
		return nested(strings.Repeat("&#32;", leading) + rendered[leading:])
	case HeadingKind:
		lvl := int(k.Level)
		if lvl < 1 {
			lvl = 1
		}
		if lvl > 6 {
			lvl = 6
		}
		return nested(strings.Repeat("#", lvl) + " " + text)
	case BulletKind:
		return nested("- " + text)
	case OrderedKind:
		return nested(fmt.Sprintf("%d. %s", seq, text))
	case TodoKind:
		mark := " "
		if k.Done {
			mark = "x"
		}
		return nested("- [" + mark + "] " + text)
	case CodeKind:
		if k.FrontMatter {
			return "---\n" + b.Text.Text + "\n---"
		}
		lang := k.Lang
		if strings.EqualFold(lang, "plain text") {
			lang = ""
		} else if k.fence.marker == 0 {
			lang = strings.ToLower(strings.ReplaceAll(lang, " ", ""))
		}
		marker := k.fence.marker
		if marker == 0 {
			marker = '`'
		}
		longest, run := 0, 0
		for _, c := range b.Text.Text {
			if c == marker {
				run++
				if run > longest {
					longest = run
				}
			} else {
				run = 0
			}
		}
		width := max(longest+1, k.fence.open, 3)
		opening := strings.Repeat(string(marker), width)
		closing := strings.Repeat(string(marker), max(width, k.fence.close))
		return opening + k.fence.beforeLang + lang + k.fence.afterLang + "\n" + b.Text.Text + "\n" + closing
	case QuoteKind:
		return indent(w.blocks(b.Children), "> ")
	case CalloutKind:
		return indent(k.Emoji+" "+w.blocks(b.Children), "> ")
	case DividerKind:
		return "---"
	case TOCKind:
		if IsTOCMarker(k.Marker) {
			return k.Marker
		}
		return "[toc]"
	case DefinitionKind:
		return nested(b.Text.Text)
	case ImageKind:
		if w.opts.KeepPaths && k.markup != "" {
			return k.markup
		}
		if !w.opts.KeepPaths && w.opts.Assets == nil {
			return ""
		}
		alt := ""
		if k.Caption != nil {
			alt = *k.Caption
		}
		link := w.image(k.Src)
		return "![" + escapeText(alt, false) + "](" + w.assetURL(link) + ")"
	case TableKind:
		return w.table(k.Table)
	case GridKind:
		return w.blocks(b.Children)
	case ColumnKind:
		return w.blocks(b.Children)
	case CountdownKind:
		return ""
	case InteractiveKindBlock:
		source := w.interactiveSource(k.Interactive)
		payload, _ := json.Marshal(map[string]any{
			"title":  k.Interactive.Title,
			"height": k.Interactive.VisibleHeight(),
			"source": source,
		})
		longest, run := 0, 0
		for _, c := range k.Interactive.Source {
			if c == '`' {
				run++
				if run > longest {
					longest = run
				}
			} else {
				run = 0
			}
		}
		fence := strings.Repeat("`", max(longest+1, 3))
		return fence + "mdxb-" + k.Interactive.Kind.String() + "\n" + string(payload) + "\n" + fence
	case FileKind:
		if url, ok := w.fileLink(k.File); ok {
			return "[" + escapeText(k.File.Name, false) + "](" + url + ")"
		}
		return escapeText(k.File.Name, false)
	case SubPageListKind:
		var lines []string
		for _, t := range w.opts.SubPages {
			lines = append(lines, "- "+escapeText(DisplayTitle(t), false))
		}
		return strings.Join(lines, "\n")
	case RawKind:
		text, link := rawSummary(k.XML)
		if link != "" && LinkOK(link) {
			label := text
			if label == "" {
				label = link
			}
			return "[" + escapeText(label, false) + "](" + strings.ReplaceAll(link, " ", "%20") + ")"
		}
		return strings.ReplaceAll(escapeText(text, false), "\n", "<br>")
	}
	return ""
}

// MarkdownInline serializes a selection without turning it into a paragraph.
func MarkdownInline(t RichText) string {
	w := writer{opts: Options{KeepPaths: true}}
	return w.rich(&t)
}

// MarkdownTableRow serializes one GFM row for copying.
func MarkdownTableRow(t *Table, row int) string {
	w := writer{opts: Options{KeepPaths: true}}
	return w.tableRow(t.Rows[row], t.NCols())
}

func escapeParagraphStart(line string) string {
	trimmed := strings.TrimLeft(line, " ")
	if trimmed == "" {
		return line
	}
	start := len(line) - len(trimmed)
	if head := strings.TrimLeft(trimmed, "#"); len(trimmed)-len(head) <= 6 && len(head) < len(trimmed) &&
		(head == "" || head[0] == ' ' || head[0] == '\t') {
		return line[:start] + "\\" + trimmed
	}
	switch trimmed[0] {
	case '>':
		return line[:start] + "\\" + trimmed
	case '-':
		if strings.Trim(strings.TrimSpace(trimmed), "-") == "" || divider(trimmed) ||
			strings.HasPrefix(trimmed, "- ") || strings.HasPrefix(trimmed, "-\t") {
			return line[:start] + "\\" + trimmed
		}
	case '+':
		if strings.HasPrefix(trimmed, "+ ") || strings.HasPrefix(trimmed, "+\t") {
			return line[:start] + "\\" + trimmed
		}
	case '=':
		if strings.Trim(strings.TrimSpace(trimmed), "=") == "" {
			return line[:start] + "\\" + trimmed
		}
	}
	if mark, _, _, ok := listItem(trimmed); ok && !mark.bullet {
		i := strings.IndexAny(trimmed, ".)")
		if i > 0 {
			return line[:start+i] + "\\" + line[start+i:]
		}
	}
	return line
}

func (w *writer) rich(t *RichText) string {
	if w.opts.KeepPaths && len(t.inlineSources) != 0 {
		var out strings.Builder
		at := 0
		for i := 0; i < len(t.inlineSources); {
			src := t.inlineSources[i]
			before := t.Slice(at, src.Start)
			out.WriteString(w.formattedRich(&before))
			start, end := src.Start, src.End
			var raw strings.Builder
			raw.WriteString(src.Markup)
			i++
			for i < len(t.inlineSources) && t.inlineSources[i].Start == end {
				next := t.inlineSources[i]
				raw.WriteString(next.Markup)
				end = next.End
				i++
			}
			part := t.Slice(start, end)
			markup := raw.String()
			// When editing joins two styled tokens or brings underscore
			// emphasis next to a word, its original delimiters may become
			// ambiguous. Re-encode that span while keeping other spellings.
			lo, hi := start, end
			if lo > 0 {
				_, n := utf8.DecodeLastRuneInString(t.Text[:lo])
				lo -= n
			}
			if hi < t.Len() {
				_, n := utf8.DecodeRuneInString(t.Text[hi:])
				hi += n
			}
			context := escapeText(t.Text[lo:start], false) + markup + escapeText(t.Text[end:hi], false)
			parsed := mdInlineWith(context, true, true)
			if parsed.Len() == hi-lo && parsed.Slice(start-lo, end-lo).sameInlineContent(part) {
				out.WriteString(markup)
			} else {
				out.WriteString(w.formattedRich(&part))
			}
			at = end
		}
		after := t.Slice(at, t.Len())
		out.WriteString(w.formattedRich(&after))
		return out.String()
	}
	return w.formattedRich(t)
}

func (w *writer) formattedRich(t *RichText) string {
	if !w.opts.KeepPaths && w.opts.Assets == nil {
		return renderInline(t, func(asset inlineAsset) (string, bool) { return "", false })
	}
	return renderInline(t, func(asset inlineAsset) (string, bool) {
		switch a := asset.(type) {
		case inlineFile:
			return w.fileLink(a.File)
		case inlineImage:
			link := w.image(a.Image.Src)
			if link == "" {
				return "", false
			}
			return w.assetURL(link), true
		}
		return "", false
	})
}

func (w *writer) fileLink(f FileBlock) (string, bool) {
	if (!w.opts.KeepPaths && w.opts.Assets == nil) || f.SrcKind != "path" {
		return "", false
	}
	link := w.image(f.Src)
	if link == "" {
		return "", false
	}
	return w.assetURL(link), true
}

func (w *writer) assetURL(link string) string {
	if strings.HasPrefix(link, "data:image/") {
		return link
	}
	prefix := ""
	if w.opts.DotRelative && link != "" && !strings.HasPrefix(link, "http://") && !strings.HasPrefix(link, "https://") {
		prefix = "./"
	}
	r := strings.NewReplacer(" ", "%20", ")", "%29", "(", "%28")
	return prefix + r.Replace(link)
}

func (w *writer) interactiveSource(i Interactive) string {
	if i.Kind != InteractiveHTML || w.opts.Assets == nil {
		return i.Source
	}
	return rewriteHTMLSources(i.Source, func(src string) (string, bool) {
		if strings.HasPrefix(src, "data:") || strings.HasPrefix(src, "blob:") ||
			strings.HasPrefix(src, "#") || strings.HasPrefix(src, "http://") || strings.HasPrefix(src, "https://") {
			return "", false
		}
		rel := w.image(src)
		if rel == "" {
			return "", false
		}
		r := strings.NewReplacer("%", "%25", " ", "%20", "#", "%23", "?", "%3F")
		return "./" + r.Replace(rel), true
	})
}

func (w *writer) tableWidget(i Interactive) string {
	clone := i
	clone.Source = w.interactiveSource(i)
	return MarkdownTableWidget(clone)
}

func (w *writer) table(t *Table) string {
	cols := 1
	for _, r := range t.Rows {
		cols = max(cols, len(r))
	}
	var lines []string
	if len(t.Rows) > 0 {
		lines = append(lines, w.tableRow(t.Rows[0], cols))
	} else {
		lines = append(lines, w.tableRow(nil, cols))
	}
	markers := make([]string, cols)
	for c := 0; c < cols; c++ {
		al := AlignLeft
		for _, r := range t.Rows {
			if c < len(r) && len(r[c].Blocks) > 0 {
				al = r[c].Blocks[0].Align
				break
			}
		}
		switch al {
		case AlignCenter:
			markers[c] = ":---:"
		case AlignRight:
			markers[c] = "---:"
		default:
			markers[c] = "---"
		}
	}
	lines = append(lines, "| "+strings.Join(markers, " | ")+" |")
	for _, r := range t.Rows[min(len(t.Rows), 1):] {
		lines = append(lines, w.tableRow(r, cols))
	}
	return strings.Join(lines, "\n")
}

func (w *writer) tableRow(row []TableCell, cols int) string {
	cells := make([]string, max(len(row), cols))
	for i, cell := range row {
		var parts []string
		for _, b := range cell.Blocks {
			var text string
			switch k := b.Kind.(type) {
			case ImageKind:
				text = w.block(b, 0)
			case InteractiveKindBlock:
				text = w.tableWidget(k.Interactive)
			default:
				text = w.rich(&b.Text)
			}
			if text != "" {
				parts = append(parts, text)
			}
		}
		cellText := strings.ReplaceAll(strings.Join(parts, "<br>"), "\n", "<br>")
		cells[i] = strings.ReplaceAll(cellText, "|", `\|`)
	}
	return "| " + strings.Join(cells, " | ") + " |"
}

// image returns the link for a local file: a copy in the assets folder, same
// name, numbered on clashes.
func (w *writer) image(src string) string {
	if w.opts.KeepPaths {
		return src
	}
	if strings.HasPrefix(src, "http://") || strings.HasPrefix(src, "https://") || strings.HasPrefix(src, "data:image/") {
		return src
	}
	if w.opts.Resolve == nil {
		w.missing++
		return ""
	}
	abs, ok := w.opts.Resolve(src)
	if !ok {
		w.missing++
		return ""
	}
	for _, f := range w.files {
		if f[0] == abs {
			return f[1]
		}
	}
	stem := strings.TrimSuffix(filepath.Base(abs), filepath.Ext(abs))
	ext := filepath.Ext(abs)
	dir := *w.opts.Assets
	rel := dir + "/" + stem + ext
	n := 1
	for {
		clash := false
		for _, f := range w.files {
			if f[1] == rel {
				clash = true
				break
			}
		}
		if !clash {
			break
		}
		n++
		rel = fmt.Sprintf("%s/%s-%d%s", dir, stem, n, ext)
	}
	w.files = append(w.files, [2]string{abs, rel})
	return rel
}

// indent prefixes every line, leaving blank lines empty.
func indent(s, prefix string) string {
	lines := strings.Split(s, "\n")
	for i, l := range lines {
		if l == "" {
			lines[i] = strings.TrimRight(prefix, " ")
		} else {
			lines[i] = prefix + l
		}
	}
	return strings.Join(lines, "\n")
}

// escapeText escapes Markdown punctuation, optionally keeping plain wiki and
// safe relative links.
func escapeText(s string, preserveLinks bool) string {
	var out strings.Builder
	out.Grow(len(s))
	at := 0
	for at < len(s) {
		rest := s[at:]
		if preserveLinks {
			if n := plainLinkLen(rest); n > 0 {
				out.WriteString(rest[:n])
				at += n
				continue
			}
		}
		c, size := utf8.DecodeRuneInString(rest)
		if strings.ContainsRune("\\*_`[]<~&$", c) || strings.HasPrefix(rest, "&#32;") {
			out.WriteByte('\\')
		}
		out.WriteRune(c)
		at += size
	}
	return out.String()
}

// plainLinkLen measures a wiki or safe relative link at the start of s.
func plainLinkLen(s string) int {
	if body, ok := strings.CutPrefix(s, "[["); ok {
		if end := strings.Index(body, "]]"); end >= 0 {
			inner := body[:end]
			bad := strings.TrimSpace(inner) == ""
			for _, c := range inner {
				if unicode.IsControl(c) || c == '<' || c == '>' || c == '\\' {
					bad = true
					break
				}
			}
			if !bad {
				return 2 + end + 2
			}
		}
	}
	body, ok := strings.CutPrefix(s, "[")
	if !ok {
		return 0
	}
	end := strings.Index(body, "](")
	if end < 0 {
		return 0
	}
	label := body[:end]
	target := body[end+2:]
	close := strings.Index(target, ")")
	if close < 0 {
		return 0
	}
	url := target[:close]
	for _, c := range label {
		if unicode.IsControl(c) || c == '<' || c == '>' || c == '\\' {
			return 0
		}
	}
	if url == "" || strings.HasPrefix(url, "/") || strings.HasPrefix(url, "\\") || strings.Contains(url, ":") {
		return 0
	}
	for _, c := range url {
		if unicode.IsControl(c) || c == '<' || c == '>' || c == '\\' {
			return 0
		}
	}
	return 1 + end + 2 + close + 1
}

// mdStyle is the subset of marks Markdown can express.
type mdStyle struct {
	bold, italic, strike, underline, code, image, equation, reference bool
	link                                                              string
}

func (m mdStyle) equal(o mdStyle) bool {
	return m.bold == o.bold && m.italic == o.italic && m.strike == o.strike &&
		m.underline == o.underline && m.code == o.code && m.image == o.image &&
		m.equation == o.equation && m.reference == o.reference && m.link == o.link
}

type inlineAsset interface{ asset() }

type inlineFile struct{ File FileBlock }
type inlineImage struct{ Image InlineImage }

func (inlineFile) asset()  {}
func (inlineImage) asset() {}

// renderInline writes one rich text as Markdown.
func renderInline(t *RichText, assets func(inlineAsset) (string, bool)) string {
	type piece struct {
		style mdStyle
		text  string
	}
	var runs []piece
	for _, sp := range t.Spans() {
		src := t.Text[sp.Start:sp.End]
		var s string
		ref, reference := sp.Marks.Inline.Reference()
		if eq, ok := sp.Marks.Inline.Equation(); ok {
			var b strings.Builder
			for range src {
				b.WriteString("$" + eq + "$")
			}
			s = b.String()
		} else if f, ok := sp.Marks.Inline.File(); ok {
			s = strings.Repeat(f.Name, utf8.RuneCountInString(src))
		} else if img, ok := sp.Marks.Inline.Image(); ok {
			markup := ""
			if u, ok := assets(inlineImage{img}); ok {
				markup = "![" + escapeText(img.Alt, false) + "](" + u + ")"
			}
			s = strings.Repeat(markup, utf8.RuneCountInString(src))
		} else if reference {
			s = strings.Repeat(ref.Markup, utf8.RuneCountInString(src))
		} else if sp.Marks.Inline != nil {
			text, link := rawSummary(sp.Marks.Inline.XML)
			_, isButton := sp.Marks.Inline.Button()
			out := text
			if isButton && link != "" && LinkOK(link) {
				out = "[" + text + "](" + link + ")"
			}
			s = strings.Repeat(out, utf8.RuneCountInString(src))
		} else {
			s = src
		}
		style := mdStyle{
			bold:      sp.Marks.Bold,
			italic:    sp.Marks.Italic,
			strike:    sp.Marks.Strike,
			underline: sp.Marks.Underline,
			code:      sp.Marks.Code,
			image:     sp.Marks.Inline.hasImage(),
			equation:  sp.Marks.Inline.hasEquation(),
			reference: reference,
		}
		if f, ok := sp.Marks.Inline.File(); ok {
			if u, ok := assets(inlineFile{f}); ok {
				style.link = u
			}
		} else if link := sp.Marks.Link; link != nil {
			_, local := LocalFileURLPath(*link)
			if LinkOK(*link) || RelativeLinkOK(*link) || local {
				style.link = *link
			}
		}
		if n := len(runs); n > 0 && runs[n-1].style.equal(style) {
			runs[n-1].text += s
		} else {
			runs = append(runs, piece{style, s})
		}
	}
	adjacent := func(a, b piece) bool {
		styled := func(p piece) bool { return p.style.bold || p.style.italic || p.style.strike }
		if !styled(a) || !styled(b) {
			return false
		}
		lastRune, _ := utf8.DecodeLastRuneInString(a.text)
		firstRune, _ := utf8.DecodeRuneInString(b.text)
		return !unicode.IsSpace(lastRune) && !unicode.IsSpace(firstRune)
	}
	htmlWrap := make([]bool, len(runs))
	for i := range runs {
		htmlWrap[i] = (i > 0 && adjacent(runs[i-1], runs[i])) || (i+1 < len(runs) && adjacent(runs[i], runs[i+1]))
	}
	var out strings.Builder
	for i, p := range runs {
		s := p.text
		if s == "" {
			continue
		}
		var body, lead, trail string
		if p.style.code {
			body, lead, trail = s, "", ""
		} else {
			trimmedLeft := strings.TrimLeftFunc(s, unicode.IsSpace)
			trimmedRight := strings.TrimRightFunc(s, unicode.IsSpace)
			lead = s[:len(s)-len(trimmedLeft)]
			trail = s[len(trimmedRight):]
			body = strings.TrimSpace(s)
		}
		if body == "" {
			out.WriteString(s)
			continue
		}
		var x string
		switch {
		case p.style.image || p.style.equation || p.style.reference:
			x = body
		case p.style.code:
			longest, run := 0, 0
			for _, c := range body {
				if c == '`' {
					run++
					if run > longest {
						longest = run
					}
				} else {
					run = 0
				}
			}
			tick := strings.Repeat("`", longest+1)
			pad := strings.HasPrefix(body, "`") || strings.HasSuffix(body, "`") ||
				(strings.HasPrefix(body, " ") && strings.HasSuffix(body, " ") && strings.TrimSpace(body) != "")
			if pad {
				x = tick + " " + body + " " + tick
			} else {
				x = tick + body + tick
			}
		default:
			x = escapeText(body, p.style.link == "")
		}
		if p.style.underline {
			x = "<u>" + x + "</u>"
		}
		if p.style.strike {
			if htmlWrap[i] {
				x = "<del>" + x + "</del>"
			} else {
				x = "~~" + x + "~~"
			}
		}
		if p.style.italic {
			if htmlWrap[i] {
				x = "<em>" + x + "</em>"
			} else {
				x = "*" + x + "*"
			}
		}
		if p.style.bold {
			if htmlWrap[i] {
				x = "<strong>" + x + "</strong>"
			} else {
				x = "**" + x + "**"
			}
		}
		if p.style.link != "" {
			r := strings.NewReplacer(" ", "%20", "(", "%28", ")", "%29")
			x = "[" + x + "](" + r.Replace(p.style.link) + ")"
		}
		out.WriteString(lead)
		out.WriteString(x)
		out.WriteString(trail)
	}
	return out.String()
}

func (in *Inline) hasImage() bool {
	_, ok := in.Image()
	return ok
}

func (in *Inline) hasEquation() bool {
	_, ok := in.Equation()
	return ok
}

// ReplaceHardBreaksInParagraph turns every internal newline into a CommonMark
// hard break (two trailing spaces). It is only applied to plain paragraphs:
// list items and quotes carry their own line prefixes.
func ReplaceHardBreaksInParagraph(s string) string {
	if !strings.Contains(s, "\n") {
		return s
	}
	var out strings.Builder
	out.Grow(len(s) + strings.Count(s, "\n")*2)
	start := 0
	for i := 0; i < len(s); i++ {
		if s[i] != '\n' {
			continue
		}
		out.WriteString(s[start:i])
		// Count trailing spaces already written.
		trailing := 0
		cur := out.String()
		for j := len(cur) - 1; j >= 0 && cur[j] == ' '; j-- {
			trailing++
		}
		if trailing < 2 {
			out.WriteString(strings.Repeat(" ", 2-trailing))
		}
		out.WriteByte('\n')
		start = i + 1
	}
	out.WriteString(s[start:])
	return out.String()
}

// LinkOK reports whether a URL is one Markdown may link to.
func LinkOK(h string) bool {
	l := strings.ToLower(h)
	for _, p := range []string{"http://", "https://", "mailto:", "ftp://"} {
		if strings.HasPrefix(l, p) {
			return true
		}
	}
	return false
}

// RelativeLinkOK reports whether a link is a document-relative file path.
func RelativeLinkOK(h string) bool {
	_, ok := SplitRelativeLink(h)
	return ok
}

// SplitRelativeLink splits `path#fragment`; an empty path means a heading in
// the current document.
func SplitRelativeLink(h string) (string, bool) {
	if h == "" || len(h) > 8192 || strings.Contains(h, "?") {
		return "", false
	}
	for _, c := range h {
		if unicode.IsControl(c) {
			return "", false
		}
	}
	path, fragment, hasFrag := strings.Cut(h, "#")
	if hasFrag {
		if fragment == "" || strings.Contains(fragment, "#") {
			return "", false
		}
	}
	if path == "" {
		return "", hasFrag
	}
	if !RelativePathOK(path) {
		return "", false
	}
	return path, true
}

// RelativePathOK reports whether a decoded relative path is safe.
func RelativePathOK(h string) bool {
	if h == "" || len(h) > 8192 || strings.HasPrefix(h, "/") || strings.HasPrefix(h, "\\") {
		return false
	}
	if strings.ContainsAny(h, "\\:") {
		return false
	}
	for _, c := range h {
		if unicode.IsControl(c) {
			return false
		}
	}
	return true
}
