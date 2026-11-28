package doc

import (
	"encoding/base64"
	"encoding/json"
	"strings"
	"unicode"
	"unicode/utf8"
)

// Off are the rules a paste target switches off (a list item does not
// re-detect its own kind, a quote no quotes, and so on).
type Off struct {
	Bullet      bool
	Ordered     bool
	Todo        bool
	Quote       bool
	CodeDivider bool
	// File marks a whole-file import: blank lines only separate blocks, and a
	// line holding just `![alt](src)` is an image.
	File bool
}

// MarkdownToBlocks parses Markdown into blocks.
func MarkdownToBlocks(text string, off Off) []*Block {
	text = strings.ReplaceAll(text, "\r\n", "\n")
	text = strings.ReplaceAll(text, "\r", "\n")
	lines := strings.Split(text, "\n")
	return mdBlocks(lines, off)
}

// MarkdownInlineLines parses one block per line, applying inline rules only
// (for pasting into a non-empty block).
func MarkdownInlineLines(text string) []*Block {
	text = strings.ReplaceAll(text, "\r\n", "\n")
	text = strings.ReplaceAll(text, "\r", "\n")
	text = strings.TrimRight(text, "\n")
	var out []*Block
	for _, l := range strings.Split(text, "\n") {
		out = append(out, NewBlock(TextKind{}, mdInlineMode(tabs(l), false)))
	}
	return out
}

// tabs collapses runs of tabs into one space.
func tabs(s string) string {
	var b strings.Builder
	prevTab := false
	for _, c := range s {
		if c != '\t' {
			b.WriteRune(c)
			prevTab = false
			continue
		}
		if !prevTab {
			b.WriteRune('\t')
		}
		prevTab = true
	}
	return strings.ReplaceAll(b.String(), "\t", " ")
}

// IndentOf counts a line's leading columns, with tabs to the next stop of 4.
func IndentOf(line string) int {
	n := 0
	for _, c := range line {
		switch c {
		case ' ':
			n++
		case '\t':
			n += 4 - n%4
		default:
			return n
		}
	}
	return n
}

// Dedent removes the first n columns of indentation.
func Dedent(line string, n int) string {
	col := 0
	for i, c := range line {
		if col >= n {
			return line[i:]
		}
		switch c {
		case ' ':
			col++
		case '\t':
			col += 4 - col%4
		default:
			return line[i:]
		}
	}
	return ""
}

// markerKind is a list marker: a bullet, or an explicit ordered number.
type markerKind struct {
	bullet  bool
	ordered bool
	number  uint32
}

// listItem parses a list item line into its marker, content column and content.
func listItem(line string) (markerKind, int, string, bool) {
	ind := IndentOf(line)
	rest := strings.TrimLeft(line, " \t")
	if rest == "" {
		return markerKind{}, 0, "", false
	}
	var mk markerKind
	var mlen int
	first, size := utf8.DecodeRuneInString(rest)
	switch first {
	case '-', '*', '+':
		mk = markerKind{bullet: true}
		mlen = size
	default:
		digits := 0
		for digits < len(rest) && rest[digits] >= '0' && rest[digits] <= '9' {
			digits++
		}
		if digits == 0 || digits > 9 {
			return markerKind{}, 0, "", false
		}
		tail := rest[digits:]
		if tail == "" {
			return markerKind{}, 0, "", false
		}
		d, dsize := utf8.DecodeRuneInString(tail)
		if d != '.' && d != ')' && d != '。' {
			return markerKind{}, 0, "", false
		}
		var n uint32
		for i := 0; i < digits; i++ {
			n = n*10 + uint32(rest[i]-'0')
		}
		mk = markerKind{ordered: true, number: n}
		mlen = digits + dsize
	}
	after := rest[mlen:]
	if after == "" {
		return markerKind{}, 0, "", false
	}
	firstSep, firstSize := utf8.DecodeRuneInString(after)
	if firstSep != ' ' && firstSep != '\t' {
		return markerKind{}, 0, "", false
	}
	markerEnd := ind + utf8.RuneCountInString(rest[:mlen])
	firstWidth := 1
	if firstSep == '\t' {
		firstWidth = 4 - markerEnd%4
	}
	columns := markerEnd
	padding := 0
	for _, c := range after {
		if c != ' ' && c != '\t' {
			break
		}
		if c == '\t' {
			columns += 4 - columns%4
		} else {
			columns++
		}
		padding += utf8.RuneLen(c)
	}
	if columns-markerEnd > 4 || padding >= len(after) {
		padding = firstSize
		columns = markerEnd + firstWidth
	}
	content := after[padding:]
	if divider(rest) {
		return markerKind{}, 0, "", false
	}
	return mk, columns, content, true
}

// divider reports whether a line is a thematic break.
func divider(line string) bool {
	var b strings.Builder
	for _, c := range strings.TrimSpace(line) {
		if c != ' ' {
			b.WriteRune(c)
		}
	}
	t := b.String()
	if t == "---" || t == "***" || t == "___" {
		return true
	}
	if len(t) > 3 && len(t) < 6 && strings.Trim(t, "-") == "" {
		return true
	}
	return false
}

// fence reports whether a line opens a fenced code block.
func fence(line string) (ch rune, n int, info string, ok bool) {
	t := strings.TrimLeft(line, " \t")
	if t == "" {
		return 0, 0, "", false
	}
	c, _ := utf8.DecodeRuneInString(t)
	if c != '`' && c != '~' {
		return 0, 0, "", false
	}
	for _, r := range t {
		if r != c {
			break
		}
		n++
	}
	if n < 3 {
		return 0, 0, "", false
	}
	return c, n, strings.TrimSpace(t[n:]), true
}

// todoPrefix strips a task-list marker.
func todoPrefix(s string) (done bool, rest string, ok bool) {
	for _, p := range []struct {
		prefix string
		done   bool
	}{
		{"[ ] ", false}, {"[] ", false}, {"【】 ", false},
		{"[x] ", true}, {"[X] ", true}, {"【x】 ", true},
	} {
		if r, found := strings.CutPrefix(s, p.prefix); found {
			return p.done, r, true
		}
	}
	return false, s, false
}

// quoteLine strips a blockquote marker.
func quoteLine(s string) (string, bool) {
	if r, ok := strings.CutPrefix(s, ">"); ok {
		return strings.TrimPrefix(r, " "), true
	}
	return "", false
}

// tableCells splits one GFM table row into cells.
func tableCells(line string) []string {
	t := strings.TrimSpace(line)
	t = strings.TrimPrefix(t, "|")
	if strings.HasSuffix(t, "|") && !strings.HasSuffix(t, `\|`) {
		t = t[:len(t)-1]
	}
	var cells []string
	var cur strings.Builder
	for i := 0; i < len(t); i++ {
		if t[i] == '\\' && i+1 < len(t) && t[i+1] == '|' {
			cur.WriteByte('|')
			i++
			continue
		}
		if t[i] == '|' {
			cells = append(cells, strings.TrimSpace(cur.String()))
			cur.Reset()
			continue
		}
		cur.WriteByte(t[i])
	}
	cells = append(cells, strings.TrimSpace(cur.String()))
	return cells
}

// TableFromHeader builds a GFM table when a typed pipe-delimited row is
// confirmed with Enter.
func TableFromHeader(line string) *Block {
	trim := strings.TrimSpace(line)
	if !strings.HasPrefix(trim, "|") || !strings.HasSuffix(trim, "|") {
		return nil
	}
	cols := len(tableCells(trim))
	if cols < 2 || cols > 64 {
		return nil
	}
	b, _, _ := pipeTable([]string{trim, "|" + strings.Repeat("---|", cols), "|" + strings.Repeat(" |", cols)}, true)
	return b
}

// mdBlocks parses physical lines into blocks.
func mdBlocks(lines []string, off Off) []*Block {
	return mdBlocksLocated(lines, off, nil)
}

func mdBlocksLocated(lines []string, off Off, locate func(*Block, int, int)) []*Block {
	var out []*Block
	i := 0
	start, count := 0, 0
	record := func() {
		if locate != nil && len(out) > count {
			locate(out[count], start, i)
		}
	}
	lastList := false
	for i < len(lines) {
		record()
		start, count = i, len(out)
		line := lines[i]
		if strings.TrimSpace(line) == "" {
			if len(out) > 0 && !off.File {
				out = append(out, TextBlock(""))
			}
			lastList = false
			i++
			continue
		}
		ind := IndentOf(line)
		body := strings.TrimSpace(line)
		if off.File && i == 0 && line == "---" {
			for end := i + 1; end < len(lines); end++ {
				if lines[end] == "---" || lines[end] == "..." {
					out = append(out, NewBlock(CodeKind{Lang: "yaml", FrontMatter: true},
						PlainRich(strings.Join(lines[i+1:end], "\n"))))
					i = end + 1
					break
				}
			}
			if i != 0 {
				continue
			}
		}
		if ch, n, info, ok := fence(line); ok && !off.CodeDivider {
			var code []string
			closing := n
			j := i + 1
			for j < len(lines) {
				if c2, n2, rest, ok2 := fence(lines[j]); ok2 && c2 == ch && n2 >= n && rest == "" {
					closing = n2
					break
				}
				code = append(code, Dedent(lines[j], ind))
				j++
			}
			bodyText := strings.Join(code, "\n")
			if kind, ok := strings.CutPrefix(info, "mdxb-"); ok {
				if it, ok := interactiveFromFence(kind, bodyText); ok {
					out = append(out, NewBlock(InteractiveKindBlock{it}, RichText{}))
					i = min(j+1, len(lines))
					lastList = false
					continue
				}
			}
			langToken := ""
			if f := strings.Fields(info); len(f) > 0 {
				langToken = f[0]
			}
			raw := strings.TrimLeft(line, " \t")[n:]
			left := strings.TrimLeftFunc(raw, unicode.IsSpace)
			layout := fenceStyle{marker: ch, open: n, close: closing, beforeLang: raw[:len(raw)-len(left)], afterLang: left[len(langToken):]}
			out = append(out, NewBlock(CodeKind{Lang: langToken, fence: layout}, PlainRich(bodyText)))
			i = min(j+1, len(lines))
			lastList = false
			continue
		}
		// A complete display formula is one equation object; an unclosed
		// delimiter stays plain text.
		open, close := "$$", "$$"
		if strings.HasPrefix(body, `\[`) {
			open, close = `\[`, `\]`
		}
		if ind <= 3 && !off.CodeDivider && strings.HasPrefix(body, open) {
			var src string
			var end int
			found := false
			if body == open {
				e := i + 1
				for e < len(lines) && strings.TrimSpace(lines[e]) != close {
					e++
				}
				if e < len(lines) {
					src = strings.Join(lines[i+1:e], "\n")
					end = e + 1
					found = true
				}
			} else if inner, ok := strings.CutPrefix(body, open); ok {
				if inner, ok := strings.CutSuffix(inner, close); ok {
					src = strings.TrimSpace(inner)
					end = i + 1
					found = true
				}
			}
			if found && strings.TrimSpace(src) != "" {
				var t RichText
				inline := DisplayEquationInline(src)
				t.Push(Obj, Marks{Inline: &inline})
				b := NewBlock(TextKind{}, t)
				b.Align = AlignCenter
				out = append(out, b)
				i = end
				lastList = false
				continue
			}
		}
		if mk, col, content, ok := listItem(line); ok && !offSkip(mk, content, off) {
			j := i + 1
			var sub []string
			for j < len(lines) {
				l := lines[j]
				if strings.TrimSpace(l) == "" {
					next := j + 1
					for next < len(lines) && strings.TrimSpace(lines[next]) == "" {
						next++
					}
					if next < len(lines) && IndentOf(lines[next]) > ind {
						for k := 0; k < next-j; k++ {
							sub = append(sub, "")
						}
						j = next
						continue
					}
					break
				}
				if IndentOf(l) <= ind {
					break
				}
				sub = append(sub, Dedent(l, min(col, IndentOf(l))))
				j++
			}
			var kind BlockKind
			text := content
			if done, rest, ok := todoPrefix(content); ok {
				kind = TodoKind{Done: done}
				text = rest
			} else if mk.bullet {
				kind = BulletKind{}
			} else {
				cont := lastList && len(out) > 0 && isOrdered(out[len(out)-1].Kind)
				var seq *uint32
				if !cont && mk.number != 1 {
					n := mk.number
					seq = &n
				}
				kind = OrderedKind{Seq: seq}
			}
			block := buildListItem(kind, text, sub, off)
			out = append(out, block)
			lastList = true
			i = j
			continue
		}
		lastList = false
		if done, rest, ok := todoPrefix(body); ok && !off.Todo {
			out = append(out, NewBlock(TodoKind{Done: done}, mdInlineMode(strings.TrimSpace(rest), off.File)))
			i++
			continue
		}
		hashes := 0
		for hashes < len(body) && body[hashes] == '#' {
			hashes++
		}
		if ind <= 3 && hashes >= 1 && hashes <= 6 && (hashes == len(body) || body[hashes] == ' ' || body[hashes] == '\t') {
			t := strings.TrimSpace(body[hashes:])
			if trimmed := strings.TrimRight(t, "#"); len(trimmed) < len(t) &&
				(trimmed == "" || strings.HasSuffix(trimmed, " ") || strings.HasSuffix(trimmed, "\t")) {
				t = strings.TrimRight(trimmed, " \t")
			}
			out = append(out, NewBlock(HeadingKind{Level: uint8(hashes)}, mdInlineMode(t, off.File)))
			i++
			continue
		}
		if divider(body) && !off.CodeDivider {
			out = append(out, NewBlock(DividerKind{}, RichText{}))
			i++
			continue
		}
		if ind <= 3 && IsTOCMarker(body) {
			out = append(out, NewBlock(TOCKind{Marker: body}, RichText{}))
			i++
			continue
		}
		if n := definitionLines(lines[i:]); n != 0 {
			out = append(out, NewBlock(DefinitionKind{}, PlainRich(strings.Join(lines[i:i+n], "\n"))))
			i += n
			continue
		}
		if _, ok := quoteLine(body); ok && !off.Quote {
			var sub []string
			for i < len(lines) {
				r, ok := quoteLine(strings.TrimSpace(lines[i]))
				if !ok {
					break
				}
				sub = append(sub, r)
				i++
			}
			kids := mdBlocks(sub, Off{Bullet: off.Bullet, Ordered: off.Ordered, Todo: off.Todo, Quote: true, CodeDivider: off.CodeDivider, File: off.File})
			if len(kids) == 0 {
				kids = []*Block{TextBlock("")}
			}
			out = append(out, NewBlock(QuoteKind{}, RichText{}).WithChildren(kids...))
			continue
		}
		if t, n, ok := pipeTable(lines[i:], off.File); ok {
			out = append(out, t)
			i += n
			continue
		}
		if off.File {
			if src, used, ok := htmlMediaBlock(lines[i:]); ok {
				out = append(out, NewBlock(InteractiveKindBlock{HTMLInteractive("", src)}, RichText{}))
				i += used
				continue
			}
			if img, w, h, used, ok := htmlImageBlockAt(lines[i:]); ok {
				b := NewBlock(ImageKind{SrcKind: img.SrcKind, Src: img.Src, Width: w, Height: h, Caption: optStr(img.Alt), markup: strings.Join(lines[i:i+used], "\n")}, RichText{})
				b.Align = AlignCenter
				out = append(out, b)
				i += used
				continue
			}
		}
		if src, alt, ok := imageLine(body); ok && off.File {
			b := NewBlock(ImageKind{SrcKind: "src", Src: unescapeImage(src), Caption: optStr(unescapeImage(alt)), markup: body}, RichText{})
			b.Align = AlignCenter
			out = append(out, b)
			i++
			continue
		}
		// A Setext underline belongs to the preceding paragraph.
		if off.File && ind <= 3 && i+1 < len(lines) {
			underline := strings.TrimSpace(lines[i+1])
			if len(underline) > 0 && IndentOf(lines[i+1]) <= 3 {
				level := uint8(0)
				switch {
				case strings.Trim(underline, "=") == "":
					level = 1
				case strings.Trim(underline, "-") == "":
					level = 2
				}
				if level != 0 {
					out = append(out, NewBlock(HeadingKind{Level: level}, mdInlineMode(body, true)))
					i += 2
					continue
				}
			}
		}
		// Consecutive plain lines belong to one paragraph: merge them into a
		// single Text block separated by `\n`, so hard breaks round-trip.
		standalone := (strings.HasPrefix(body, "$$") || strings.HasPrefix(body, `\[`)) && !off.CodeDivider
		para := []string{tabs(line)}
		i++
		if !standalone {
			for i < len(lines) {
				next := lines[i]
				if strings.TrimSpace(next) == "" {
					break
				}
				nind := IndentOf(next)
				nbody := strings.TrimSpace(next)
				if nind < 4 {
					if IsTOCMarker(nbody) || definitionLines(lines[i:]) != 0 {
						break
					}
					if _, _, _, ok := fence(next); ok {
						break
					}
					h := 0
					for h < len(nbody) && nbody[h] == '#' {
						h++
					}
					if h >= 1 && h <= 6 && (h == len(nbody) || nbody[h] == ' ' || nbody[h] == '\t') {
						break
					}
					if divider(nbody) {
						break
					}
					if _, ok := quoteLine(nbody); ok {
						break
					}
					if _, _, _, ok := listItem(next); ok {
						break
					}
					if _, _, ok := todoPrefix(nbody); ok && !off.Todo {
						break
					}
					if _, _, ok := pipeTable(lines[i:], off.File); ok {
						break
					}
					if !off.CodeDivider && (strings.HasPrefix(nbody, "$$") || strings.HasPrefix(nbody, `\[`)) {
						break
					}
					if off.File {
						if _, _, ok := imageLine(nbody); ok {
							break
						}
					}
				}
				para = append(para, tabs(next))
				i++
			}
		}
		merged := mergeParagraphLines(para)
		out = append(out, NewBlock(TextKind{}, mdInlineMode(merged, off.File)))
	}
	record()
	return out
}

func isOrdered(k BlockKind) bool {
	_, ok := k.(OrderedKind)
	return ok
}

// offSkip reports whether the paste target asked for this marker to stay text.
func offSkip(mk markerKind, content string, off Off) bool {
	_, _, isTodo := todoPrefix(content)
	if isTodo {
		return off.Todo
	}
	if mk.bullet {
		return off.Bullet
	}
	return off.Ordered
}

// buildListItem applies the Typora-style merge of plain continuation lines.
func buildListItem(kind BlockKind, text string, sub []string, off Off) *Block {
	var contLines []string
	var childLines []string
	contLines = append(contLines, strings.TrimRight(text, " \t"))
	for _, s := range sub {
		trimmed := strings.TrimSpace(s)
		isBlockStart := IndentOf(s) >= 4 || hasFence(s) || isHeadingText(trimmed) ||
			divider(trimmed) || isQuote(trimmed) || isListItemLine(s) || isPipeTable([]string{s}, off.File) ||
			(off.File && isImageLine(trimmed))
		if isBlockStart {
			childLines = append(childLines, s)
		} else {
			contLines = append(contLines, strings.TrimRight(s, " \t"))
		}
	}
	merged := strings.Join(contLines, "\n")
	block := NewBlock(kind, mdInlineMode(merged, off.File))
	if len(childLines) > 0 {
		block.Children = mdBlocks(childLines, off)
	}
	return block
}

func hasFence(s string) bool {
	_, _, _, ok := fence(s)
	return ok
}

func isHeadingText(t string) bool {
	h := 0
	for h < len(t) && t[h] == '#' {
		h++
	}
	return h >= 1 && h <= 9 && strings.ContainsRune(t, ' ')
}

func isQuote(t string) bool {
	_, ok := quoteLine(t)
	return ok
}

func isListItemLine(s string) bool {
	_, _, _, ok := listItem(s)
	return ok
}

func isImageLine(s string) bool {
	_, _, ok := imageLine(s)
	return ok
}

func isPipeTable(lines []string, file bool) bool {
	_, _, ok := pipeTable(lines, file)
	return ok
}

// mergeParagraphLines merges physical paragraph lines into one text: trailing
// spaces are format, not content, so every line is trimmed and joined with a
// plain newline.
func mergeParagraphLines(lines []string) string {
	if len(lines) == 0 {
		return ""
	}
	var b strings.Builder
	for i, l := range lines {
		if i > 0 {
			b.WriteByte('\n')
		}
		b.WriteString(strings.TrimRight(l, " \t"))
	}
	return b.String()
}

// pipeTable parses a GFM table starting at lines[0].
func pipeTable(lines []string, file bool) (*Block, int, bool) {
	if len(lines) < 2 {
		return nil, 0, false
	}
	head := lines[0]
	if !strings.Contains(head, "|") {
		return nil, 0, false
	}
	seps := tableCells(lines[1])
	if len(seps) == 0 || !strings.Contains(lines[1], "-") {
		return nil, 0, false
	}
	for _, c := range seps {
		c = strings.Trim(c, ":")
		if c == "" || strings.Trim(c, "-") != "" {
			return nil, 0, false
		}
	}
	header := tableCells(head)
	ncols := len(header)
	if ncols != len(seps) {
		return nil, 0, false
	}
	aligns := make([]Align, ncols)
	for i, c := range seps {
		left := strings.HasPrefix(c, ":")
		right := strings.HasSuffix(c, ":")
		switch {
		case left && right:
			aligns[i] = AlignCenter
		case right:
			aligns[i] = AlignRight
		default:
			aligns[i] = AlignLeft
		}
	}
	n := 2
	rows := [][]string{header}
	for n < len(lines) {
		l := lines[n]
		if strings.TrimSpace(l) == "" || !strings.Contains(l, "|") {
			break
		}
		rows = append(rows, tableCells(l))
		n++
	}
	grid := make([][]TableCell, len(rows))
	for r, row := range rows {
		grid[r] = make([]TableCell, ncols)
		for c := 0; c < ncols; c++ {
			value := ""
			if c < len(row) {
				value = row[c]
			}
			var b *Block
			switch {
			case file && strings.Contains(value, "<markdownx-interactive data-base64=\""):
				var blocks []*Block
				for _, piece := range strings.Split(value, "<br>") {
					blocks = append(blocks, tableCellBlock(piece, file))
				}
				grid[r][c] = TableCell{Blocks: blocks, ColSpan: 1, RowSpan: 1}
				continue
			default:
				b = tableCellBlock(value, file)
			}
			b.Align = aligns[c]
			grid[r][c] = TableCell{Blocks: []*Block{b}, ColSpan: 1, RowSpan: 1}
		}
	}
	widths := make([]float32, ncols)
	for i := range widths {
		widths[i] = 200
	}
	t := &Table{ColWidths: widths, HeaderRows: 1, Rows: grid}
	t.Repair()
	return NewBlock(TableKind{Table: t}, RichText{}), n, true
}

func tableCellBlock(value string, file bool) *Block {
	if file {
		if w, ok := tableWidgetFromCell(value); ok {
			return NewBlock(InteractiveKindBlock{w}, RichText{})
		}
		if src, used, ok := htmlMediaBlock([]string{value}); ok && used == 1 {
			return NewBlock(InteractiveKindBlock{HTMLInteractive("", src)}, RichText{})
		}
		if img, w, h, ok := htmlImageLine(value); ok {
			return NewBlock(ImageKind{SrcKind: img.SrcKind, Src: img.Src, Width: w, Height: h, Caption: optStr(img.Alt)}, RichText{})
		}
		if src, alt, ok := imageLine(value); ok {
			return NewBlock(ImageKind{SrcKind: "src", Src: unescapeImage(src), Caption: optStr(unescapeImage(alt))}, RichText{})
		}
	}
	return NewBlock(TextKind{}, mdInlineMode(value, file))
}

// tableWidgetFromCell decodes an inert widget wrapper.
func tableWidgetFromCell(value string) (Interactive, bool) {
	enc, ok := strings.CutPrefix(value, `<markdownx-interactive data-base64="`)
	if !ok {
		return Interactive{}, false
	}
	enc, ok = strings.CutSuffix(enc, `"></markdownx-interactive>`)
	if !ok {
		return Interactive{}, false
	}
	raw, err := base64.StdEncoding.DecodeString(enc)
	if err != nil {
		return Interactive{}, false
	}
	var payload struct {
		Kind   string `json:"kind"`
		Title  string `json:"title"`
		Height uint32 `json:"height"`
		Source string `json:"source"`
	}
	if err := json.Unmarshal(raw, &payload); err != nil {
		return Interactive{}, false
	}
	if payload.Source == "" || (payload.Kind != "html" && payload.Kind != "canvas") {
		return Interactive{}, false
	}
	h := payload.Height
	if h == 0 {
		h = 360
	}
	return Interactive{Kind: ParseInteractiveType(payload.Kind), Title: payload.Title, Height: h, Source: payload.Source}, true
}

// interactiveFromFence reads a `mdxb-*` fenced widget.
func interactiveFromFence(kind, body string) (Interactive, bool) {
	var t InteractiveType
	switch kind {
	case "canvas":
		t = InteractiveCanvas
	case "html":
		t = InteractiveHTML
	default:
		return Interactive{}, false
	}
	var payload struct {
		Title  string `json:"title"`
		Height uint32 `json:"height"`
		Source string `json:"source"`
	}
	if err := json.Unmarshal([]byte(body), &payload); err != nil {
		return Interactive{}, false
	}
	if payload.Source == "" {
		return Interactive{}, false
	}
	h := payload.Height
	if h == 0 {
		h = 360
	}
	return Interactive{Kind: t, Title: payload.Title, Height: h, Source: payload.Source}, true
}

func optStr(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}
