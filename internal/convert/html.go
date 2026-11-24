package convert

import (
	"fmt"
	"html"
	"strings"
	"unicode"
)

// HTML reads an HTML page or fragment as Markdown.
func HTML(data []byte) (string, error) {
	md := htmlToMarkdown(string(data))
	if strings.TrimSpace(md) == "" {
		return "", fmt.Errorf("html: 正文为空")
	}
	return md + "\n", nil
}

// htmlToMarkdown converts an HTML fragment to Markdown.
//
// It is a small forgiving reader: the block and inline tags a document export
// produces become their Markdown, and anything else keeps only its text.
func htmlToMarkdown(src string) string {
	p := &htmlParser{s: src}
	p.run()
	out := html.UnescapeString(p.out.String())
	out = blankRuns(out)
	return strings.TrimSpace(out)
}

// blankRuns collapses runs of three or more newlines into a paragraph break.
func blankRuns(s string) string {
	for strings.Contains(s, "\n\n\n") {
		s = strings.ReplaceAll(s, "\n\n\n", "\n\n")
	}
	// A list item must not be followed by a blank line inside its list.
	return s
}

type listCtx struct {
	ordered bool
	n       int
}

type htmlParser struct {
	s   string
	at  int
	out strings.Builder

	lists []*listCtx
	href  string

	skipDepth int
	skipTag   string

	preDepth int
	preLang  string

	// quotes holds the output length at the start of each open blockquote.
	quotes []int

	// table state
	tableRows [][]string
	row       []string
	cell      strings.Builder
	inCell    bool
}

func (p *htmlParser) run() {
	for p.at < len(p.s) {
		if p.s[p.at] == '<' && p.tag() {
			continue
		}
		p.text(p.s[p.at])
		p.at++
	}
}

func (p *htmlParser) tag() bool {
	if strings.HasPrefix(p.s[p.at:], "<!--") {
		if end := strings.Index(p.s[p.at:], "-->"); end >= 0 {
			p.at += end + 3
			return true
		}
		p.at = len(p.s)
		return true
	}
	gt := strings.IndexByte(p.s[p.at:], '>')
	if gt < 0 {
		return false
	}
	raw := p.s[p.at+1 : p.at+gt]
	p.at += gt + 1
	closing := strings.HasPrefix(raw, "/")
	name := strings.ToLower(tagName(raw))
	selfClosing := strings.HasSuffix(strings.TrimSpace(raw), "/")

	if p.skipDepth > 0 {
		if name == p.skipTag {
			if closing {
				p.skipDepth--
			} else if !selfClosing {
				p.skipDepth++
			}
		}
		return true
	}

	switch name {
	case "script", "style", "head", "svg", "noscript", "template":
		if !closing && !selfClosing {
			p.skipTag = name
			p.skipDepth = 1
		}
		return true
	}

	// Inside a table cell, text accumulates separately. The cell's own tags
	// still fall through so the row can be closed.
	if p.inCell && name != "th" && name != "td" && name != "tr" && name != "table" {
		p.cellTag(name, raw, closing)
		return true
	}

	switch name {
	case "h1", "h2", "h3", "h4", "h5", "h6":
		if closing {
			p.blockEnd()
		} else {
			p.blockStart()
			p.out.WriteString(strings.Repeat("#", int(name[1]-'0')))
			p.out.WriteByte(' ')
		}
	case "p", "div", "section", "article", "header", "footer", "figure", "figcaption", "main":
		if closing {
			p.blockEnd()
		} else {
			p.blockStart()
		}
	case "br":
		p.out.WriteString("  \n")
	case "hr":
		p.blockStart()
		p.out.WriteString("---\n\n")
	case "strong", "b":
		p.out.WriteString("**")
	case "em", "i":
		p.out.WriteByte('*')
	case "del", "s", "strike":
		p.out.WriteString("~~")
	case "u":
		if closing {
			p.out.WriteString("</u>")
		} else {
			p.out.WriteString("<u>")
		}
	case "code":
		if p.preDepth == 0 {
			p.out.WriteByte('`')
		}
	case "pre":
		if closing {
			p.preDepth--
			lang := p.preLang
			p.preLang = ""
			p.out.WriteString("\n```\n\n")
			_ = lang
		} else {
			p.blockStart()
			p.preLang = preLangOf(raw)
			if p.preLang == "" {
				p.preLang = p.lookAheadLang()
			}
			p.out.WriteString("```" + p.preLang + "\n")
			p.preDepth++
		}
	case "blockquote":
		if closing {
			p.closeQuote()
		} else {
			p.blockStart()
			p.quotes = append(p.quotes, p.out.Len())
		}
	case "ul", "ol":
		if closing {
			if n := len(p.lists); n > 0 {
				p.lists = p.lists[:n-1]
			}
			p.blockEnd()
		} else {
			p.blockStart()
			start := 1
			if v, ok := attrOf(raw, "start"); ok {
				if n, err := parseInt(v); err == nil {
					start = n
				}
			}
			p.lists = append(p.lists, &listCtx{ordered: name == "ol", n: start - 1})
		}
	case "li":
		if closing {
			p.out.WriteByte('\n')
		} else {
			p.lineStart()
			indent := strings.Repeat("  ", max(len(p.lists)-1, 0))
			if n := len(p.lists); n > 0 {
				l := p.lists[n-1]
				if l.ordered {
					l.n++
					p.out.WriteString(indent + itoa(l.n) + ". ")
				} else {
					p.out.WriteString(indent + "- ")
				}
			} else {
				p.out.WriteString("- ")
			}
		}
	case "table":
		if closing {
			p.flushTable()
		} else {
			p.blockStart()
			p.tableRows = nil
			p.row = nil
		}
	case "tr":
		if closing {
			if len(p.row) > 0 {
				p.tableRows = append(p.tableRows, p.row)
				p.row = nil
			}
		}
	case "th", "td":
		if closing {
			p.row = append(p.row, strings.TrimSpace(p.cell.String()))
			p.cell.Reset()
			p.inCell = false
		} else {
			p.inCell = true
			p.cell.Reset()
		}
	case "a":
		if closing {
			if p.href != "" {
				p.out.WriteString("](" + p.href + ")")
				p.href = ""
			}
		} else if h, ok := attrOf(raw, "href"); ok && h != "" && !strings.HasPrefix(h, "javascript:") {
			p.href = h
			p.out.WriteByte('[')
		}
	case "img":
		src, _ := attrOf(raw, "src")
		alt, _ := attrOf(raw, "alt")
		if src != "" {
			p.out.WriteString("![" + alt + "](" + src + ")")
		}
	}
	return true
}

// closeQuote prefixes the blockquote's body with `> `.
func (p *htmlParser) closeQuote() {
	if len(p.quotes) == 0 {
		return
	}
	start := p.quotes[len(p.quotes)-1]
	p.quotes = p.quotes[:len(p.quotes)-1]
	full := p.out.String()
	head, body := full[:start], full[start:]
	p.out.Reset()
	p.out.WriteString(head)
	lines := strings.Split(strings.TrimRight(body, "\n"), "\n")
	for i, l := range lines {
		if strings.TrimSpace(l) == "" {
			p.out.WriteString(">\n")
			continue
		}
		p.out.WriteString("> " + l)
		if i < len(lines)-1 {
			p.out.WriteByte('\n')
		}
	}
	p.out.WriteString("\n\n")
}

// cellTag handles the tags inside a table cell: only inline meaning is kept.
func (p *htmlParser) cellTag(name, raw string, closing bool) {
	switch name {
	case "p", "div", "br":
		if p.cell.Len() > 0 {
			p.cell.WriteString("<br>")
		}
	case "a":
		if closing {
			if h := p.href; h != "" {
				p.cell.WriteString("](" + h + ")")
				p.href = ""
			}
		} else if h, ok := attrOf(raw, "href"); ok && h != "" {
			p.href = h
			p.cell.WriteByte('[')
		}
	case "img":
		src, _ := attrOf(raw, "src")
		alt, _ := attrOf(raw, "alt")
		if src != "" {
			p.cell.WriteString("![" + alt + "](" + src + ")")
		}
	case "strong", "b":
		if !closing {
			p.cell.WriteString("**")
		}
	case "em", "i":
		if !closing {
			p.cell.WriteByte('*')
		}
	}
}

func (p *htmlParser) flushTable() {
	if len(p.tableRows) == 0 {
		return
	}
	cols := 0
	for _, r := range p.tableRows {
		cols = max(cols, len(r))
	}
	if cols == 0 {
		return
	}
	for i, r := range p.tableRows {
		for len(r) < cols {
			r = append(r, "")
		}
		cells := make([]string, cols)
		for c, v := range r {
			cells[c] = strings.ReplaceAll(v, "|", `\|`)
		}
		p.out.WriteString("| " + strings.Join(cells, " | ") + " |\n")
		if i == 0 {
			p.out.WriteString("|" + strings.Repeat(" --- |", cols) + "\n")
		}
	}
	p.out.WriteByte('\n')
}

// text writes one character of body text.
func (p *htmlParser) text(r byte) {
	if p.skipDepth > 0 {
		return
	}
	if p.inCell {
		if r == '\n' || r == '\r' || r == '\t' {
			if b := p.cell.String(); len(b) > 0 && !strings.HasSuffix(b, " ") {
				p.cell.WriteByte(' ')
			}
			return
		}
		p.cell.WriteByte(r)
		return
	}
	switch r {
	case '\n', '\r', '\t':
		if p.preDepth > 0 {
			p.out.WriteByte(r)
			return
		}
		if b := p.out.String(); len(b) > 0 && !strings.HasSuffix(b, " ") && !strings.HasSuffix(b, "\n") {
			p.out.WriteByte(' ')
		}
		return
	}
	p.out.WriteByte(r)
}

func (p *htmlParser) blockStart() {
	s := p.out.String()
	if s == "" || strings.HasSuffix(s, "\n\n") {
		return
	}
	if strings.HasSuffix(s, "\n") {
		p.out.WriteByte('\n')
		return
	}
	p.out.WriteString("\n\n")
}

func (p *htmlParser) blockEnd() {
	s := p.out.String()
	if s == "" {
		return
	}
	if strings.HasSuffix(s, "\n\n") {
		return
	}
	if strings.HasSuffix(s, "\n") {
		p.out.WriteByte('\n')
		return
	}
	p.out.WriteString("\n\n")
}

// lineStart moves to a new line without leaving a blank one.
func (p *htmlParser) lineStart() {
	s := p.out.String()
	if s == "" {
		return
	}
	if strings.HasSuffix(s, "\n") {
		return
	}
	p.out.WriteByte('\n')
}

// lookAheadLang finds the language class of the <code> element that opens
// right after a <pre>, which is where exports put it.
func (p *htmlParser) lookAheadLang() string {
	rest := p.s[p.at:]
	i := strings.Index(strings.ToLower(rest), "<code")
	if i < 0 {
		return ""
	}
	gt := strings.IndexByte(rest[i:], '>')
	if gt < 0 {
		return ""
	}
	return preLangOf(rest[i+1 : i+gt])
}

// preLangOf reads the language class of the code element inside a <pre>.
func preLangOf(raw string) string {
	if cls, ok := attrOf(raw, "class"); ok {
		for _, part := range strings.Fields(cls) {
			if v, ok := strings.CutPrefix(part, "language-"); ok {
				return v
			}
		}
	}
	return ""
}

func tagName(raw string) string {
	raw = strings.TrimPrefix(raw, "/")
	end := len(raw)
	for i := 0; i < len(raw); i++ {
		c := raw[i]
		if c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '/' {
			end = i
			break
		}
	}
	return raw[:end]
}

// attrOf reads one attribute out of a raw tag body.
func attrOf(raw, key string) (string, bool) {
	lower := strings.ToLower(raw)
	at := 0
	for {
		k := strings.Index(lower[at:], strings.ToLower(key))
		if k < 0 {
			return "", false
		}
		i := at + k
		j := i + len(key)
		for j < len(raw) && (raw[j] == ' ' || raw[j] == '\t') {
			j++
		}
		if j >= len(raw) || raw[j] != '=' {
			at = j
			if at >= len(lower) {
				return "", false
			}
			continue
		}
		j++
		for j < len(raw) && (raw[j] == ' ' || raw[j] == '\t') {
			j++
		}
		quote := byte(0)
		if j < len(raw) && (raw[j] == '"' || raw[j] == '\'') {
			quote = raw[j]
			j++
		}
		start := j
		if quote != 0 {
			for j < len(raw) && raw[j] != quote {
				j++
			}
		} else {
			for j < len(raw) && raw[j] != ' ' && raw[j] != '>' && raw[j] != '\t' {
				j++
			}
		}
		return html.UnescapeString(raw[start:j]), true
	}
}

func parseInt(s string) (int, error) {
	n := 0
	s = strings.TrimSpace(s)
	if s == "" {
		return 0, errNotNumber
	}
	for _, r := range s {
		if !unicode.IsDigit(r) {
			return 0, errNotNumber
		}
		n = n*10 + int(r-'0')
	}
	return n, nil
}

var errNotNumber = &parseError{"not a number"}

type parseError struct{ s string }

func (e *parseError) Error() string { return e.s }

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	neg := n < 0
	if neg {
		n = -n
	}
	var b [20]byte
	i := len(b)
	for n > 0 {
		i--
		b[i] = byte('0' + n%10)
		n /= 10
	}
	if neg {
		i--
		b[i] = '-'
	}
	return string(b[i:])
}
