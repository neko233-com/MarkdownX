package doc

import (
	"strings"
	"unicode"
	"unicode/utf8"
)

// DisplayEquationInline builds a block-level `$$` equation object.
func DisplayEquationInline(src string) Inline {
	return Inline{Kind: EquationInline{Source: src}, XML: `<latex display="block">` + src + `</latex>`}
}

// EquationInlineObj builds an inline `$` equation object.
func EquationInlineObj(src string) Inline {
	return Inline{Kind: EquationInline{Source: src}, XML: `<latex>` + src + `</latex>`}
}

// ImageInlineObj builds an inline image object.
func ImageInlineObj(img InlineImage) Inline {
	return Inline{
		Kind: ImageInline{Image: img},
		XML:  `<image ` + attrIf("src-kind", img.SrcKind) + `src="` + img.Src + `" alt="` + img.Alt + `"/>`,
	}
}

func attrIf(name, v string) string {
	if v == "" {
		return ""
	}
	return name + `="` + v + `" `
}

// MdInline parses inline Markdown without whole-file rules.
func MdInline(s string) RichText { return mdInlineMode(s, false) }

func mdInlineMode(s string, file bool) RichText { return mdInlineWith(s, file, true) }

// findUnescaped finds a needle not preceded by an odd run of backslashes.
func findUnescaped(s, needle string) (int, bool) {
	from := 0
	for {
		k := strings.Index(s[from:], needle)
		if k < 0 {
			return 0, false
		}
		at := from + k
		slashes := 0
		for j := at - 1; j >= 0 && s[j] == '\\'; j-- {
			slashes++
		}
		if slashes%2 == 0 {
			return at, true
		}
		from = at + max(len(needle), 1)
	}
}

// findRun finds a delimiter run exactly equal to delim, not part of a longer
// run of ch.
func findRun(s, delim string, ch byte, escapes bool) (int, bool) {
	from := 0
	for {
		k := strings.Index(s[from:], delim)
		if k < 0 {
			return 0, false
		}
		at := from + k
		beforeOK := at == 0 || s[at-1] != ch
		afterOK := at+len(delim) >= len(s) || s[at+len(delim)] != ch
		escaped := false
		if escapes {
			slashes := 0
			for j := at - 1; j >= 0 && s[j] == '\\'; j-- {
				slashes++
			}
			escaped = slashes%2 == 1
		}
		if at > 0 && beforeOK && afterOK && !escaped {
			return at, true
		}
		from = at + max(len(delim), 1)
		for from < len(s) && s[from] == ch {
			from++
		}
	}
}

// markdownMathAt reads inline math at the start of s.
func markdownMathAt(s string, prev rune, hasPrev bool) (inner string, used int, display bool, ok bool) {
	if !strings.HasPrefix(s, "$") || (hasPrev && prev == '$') {
		return "", 0, false, false
	}
	for _, delim := range []string{"$$", "$"} {
		if !strings.HasPrefix(s, delim) {
			continue
		}
		body := s[len(delim):]
		if strings.HasPrefix(body, "$") {
			continue
		}
		end, found := findUnescaped(body, delim)
		if !found {
			continue
		}
		inner := body[:end]
		var after rune
		hasAfter := false
		if end+len(delim) < len(body) {
			after, _ = utf8.DecodeRuneInString(body[end+len(delim):])
			hasAfter = true
		}
		if strings.TrimSpace(inner) == "" || strings.ContainsRune(inner, '\n') ||
			unicode.IsSpace(rune(inner[0])) || unicode.IsSpace(lastRune(inner)) {
			continue
		}
		if hasAfter && after == '$' {
			continue
		}
		if delim == "$" && hasAfter && after >= '0' && after <= '9' {
			continue
		}
		return inner, len(delim)*2 + end, delim == "$$", true
	}
	return "", 0, false, false
}

func lastRune(s string) rune {
	r, _ := utf8.DecodeLastRuneInString(s)
	return r
}

// linkAt parses `[label](url "title")` at the start of s.
func linkAt(s string, file bool) (label, url string, used int, ok bool) {
	if file {
		probe := "!" + s
		src, alt, usedImg, ok := imageAt(probe)
		if !ok {
			return "", "", 0, false
		}
		u := unescapeImage(src)
		if !(LinkOK(u) || strings.HasPrefix(u, "www.") || RelativeLinkOK(u)) {
			return "", "", 0, false
		}
		return alt, u, usedImg - 1, true
	}
	if !strings.HasPrefix(s, "[") {
		return "", "", 0, false
	}
	depth := 0
	closeAt := -1
	for i := 0; i < len(s); i++ {
		switch s[i] {
		case '[':
			depth++
		case ']':
			depth--
			if depth == 0 {
				closeAt = i
			}
		}
		if closeAt >= 0 {
			break
		}
	}
	if closeAt < 0 {
		return "", "", 0, false
	}
	rest := s[closeAt+1:]
	inner, ok := strings.CutPrefix(rest, "(")
	if !ok {
		return "", "", 0, false
	}
	end := strings.Index(inner, ")")
	if end < 0 {
		return "", "", 0, false
	}
	target := strings.TrimSpace(inner[:end])
	fields := strings.Fields(target)
	u := ""
	if len(fields) > 0 {
		u = strings.Trim(fields[0], "<>")
	}
	if u == "" || !(LinkOK(u) || strings.HasPrefix(u, "www.")) {
		return "", "", 0, false
	}
	return s[1:closeAt], u, closeAt + 1 + 1 + end + 1, true
}

// imageAt parses `![alt](src "title")` at the start of s.
func imageAt(s string) (src, alt string, used int, ok bool) {
	if !strings.HasPrefix(s, "![") {
		return "", "", 0, false
	}
	depth := 0
	closeAt := -1
	for i := 1; i < len(s); i++ {
		if s[i] == '\\' {
			i++
			continue
		}
		switch s[i] {
		case '[':
			depth++
		case ']':
			depth--
			if depth == 0 {
				closeAt = i
			}
		}
		if closeAt >= 0 {
			break
		}
	}
	if closeAt < 0 {
		return "", "", 0, false
	}
	inner, ok := strings.CutPrefix(s[closeAt+1:], "(")
	if !ok {
		return "", "", 0, false
	}
	end := strings.Index(inner, ")")
	if end < 0 {
		return "", "", 0, false
	}
	target := strings.TrimSpace(inner[:end])
	fields := strings.Fields(target)
	if len(fields) == 0 {
		return "", "", 0, false
	}
	u := strings.Trim(fields[0], "<>")
	if u == "" {
		return "", "", 0, false
	}
	return u, s[2:closeAt], closeAt + 1 + 1 + end + 1, true
}

// imageLine matches a line that holds only an image.
func imageLine(s string) (src, alt string, ok bool) {
	s = strings.TrimSpace(s)
	src, alt, used, ok := imageAt(s)
	if !ok || used != len(s) {
		return "", "", false
	}
	return src, alt, true
}

// unescapeImage removes backslash escapes from an image path or alt.
func unescapeImage(s string) string {
	var b strings.Builder
	for i := 0; i < len(s); i++ {
		if s[i] == '\\' && i+1 < len(s) && strings.ContainsRune(`\[]()!#*_~<>"&% `, rune(s[i+1])) {
			b.WriteByte(s[i+1])
			i++
			continue
		}
		b.WriteByte(s[i])
	}
	return b.String()
}

// percentDecode turns %XX escapes into bytes.
func percentDecode(s string) string {
	if !strings.Contains(s, "%") {
		return s
	}
	var b strings.Builder
	for i := 0; i < len(s); i++ {
		if s[i] == '%' && i+2 < len(s) && isHex(s[i+1]) && isHex(s[i+2]) {
			b.WriteByte(unhex(s[i+1])<<4 | unhex(s[i+2]))
			i += 2
			continue
		}
		b.WriteByte(s[i])
	}
	return b.String()
}

func isHex(c byte) bool {
	return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')
}

func unhex(c byte) byte {
	switch {
	case c >= '0' && c <= '9':
		return c - '0'
	case c >= 'a' && c <= 'f':
		return c - 'a' + 10
	default:
		return c - 'A' + 10
	}
}

// isBoundary reports whether a rune may sit before a bounded delimiter.
func isBoundary(r rune, ok bool) bool {
	return !ok || unicode.IsSpace(r)
}

// mdInlineWith parses inline Markdown into runs.
func mdInlineWith(s string, file, autolink bool) RichText {
	var out RichText
	var plain strings.Builder
	flush := func() {
		if plain.Len() > 0 {
			out.Push(plain.String(), Marks{})
			plain.Reset()
		}
	}
	i := 0
	for i < len(s) {
		rest := s[i:]
		c, size := utf8.DecodeRuneInString(rest)
		var prev rune
		hasPrev := i > 0
		if hasPrev {
			prev = lastRune(s[:i])
		}
		if file && c == '\\' && len(rest) > 1 {
			next, nsize := utf8.DecodeRuneInString(rest[1:])
			if next < 128 && isASCIIPunct(byte(next)) {
				plain.WriteRune(next)
				i += 1 + nsize
				continue
			}
		}
		if file && c == '!' {
			if src, alt, used, ok := imageAt(rest); ok {
				flush()
				inline := ImageInlineObj(InlineImage{SrcKind: "src", Src: unescapeImage(src), Alt: unescapeImage(alt)})
				out.Push(Obj, Marks{Inline: &inline})
				i += used
				continue
			}
		}
		if file && c == '<' {
			if img, _, _, used, ok := htmlImageAt(rest); ok {
				flush()
				inline := ImageInlineObj(img)
				out.Push(Obj, Marks{Inline: &inline})
				i += used
				continue
			}
			if used, ok := inertHTMLRegionEnd(rest); ok {
				plain.WriteString(rest[:used])
				i += used
				continue
			}
		}
		if file {
			if strings.HasPrefix(rest, "&#32;") {
				plain.WriteByte(' ')
				i += 5
				continue
			}
			for _, tag := range []string{"<br>", "<br/>", "<br />"} {
				if strings.HasPrefix(rest, tag) {
					plain.WriteByte('\n')
					i += len(tag)
					c = 0
					break
				}
			}
			if c == 0 {
				continue
			}
			tagged := false
			for _, wrap := range []struct {
				open, close string
				style       int
			}{
				{"<u>", "</u>", 0},
				{"<strong>", "</strong>", 1},
				{"<em>", "</em>", 2},
				{"<del>", "</del>", 3},
			} {
				body, ok := strings.CutPrefix(rest, wrap.open)
				if !ok {
					continue
				}
				end, ok := findUnescaped(body, wrap.close)
				if !ok {
					continue
				}
				flush()
				t := mdInlineWith(body[:end], file, autolink)
				switch wrap.style {
				case 0:
					t.UpdateMarks(0, t.Len(), func(m *Marks) { m.Underline = true })
				case 1:
					t.UpdateMarks(0, t.Len(), func(m *Marks) { m.Bold = true })
				case 2:
					t.UpdateMarks(0, t.Len(), func(m *Marks) { m.Italic = true })
				default:
					t.UpdateMarks(0, t.Len(), func(m *Marks) { m.Strike = true })
				}
				out.Append(t)
				i += len(wrap.open) + end + len(wrap.close)
				tagged = true
				break
			}
			if tagged {
				continue
			}
			if used, ok := inertHTMLTagEnd(rest); ok && strings.Contains(strings.ToLower(rest[:used]), "<img") {
				plain.WriteString(rest[:used])
				i += used
				continue
			}
		}
		// Code span.
		if c == '`' {
			n := 0
			for n < len(rest) && rest[n] == '`' {
				n++
			}
			delim := rest[:n]
			if end, ok := findRun(rest[n:], delim, '`', false); ok {
				inner := rest[n : n+end]
				if len(inner) > 2 && strings.HasPrefix(inner, " ") && strings.HasSuffix(inner, " ") &&
					(!file || strings.TrimSpace(inner) != "") {
					inner = inner[1 : len(inner)-1]
				}
				if inner != "" {
					flush()
					out.Push(inner, Marks{Code: true})
					i += n + end + n
					continue
				}
			}
			plain.WriteString(delim)
			i += n
			continue
		}
		// Equations.
		if c == '$' {
			if inner, used, _, ok := markdownMathAt(rest, prev, hasPrev); ok {
				flush()
				inline := EquationInlineObj(inner)
				out.Push(Obj, Marks{Inline: &inline})
				i += used
				continue
			}
		}
		// Links.
		if c == '[' {
			if label, url, used, ok := linkAt(rest, file); ok {
				flush()
				t := mdInlineWith(label, file, false)
				if t.IsEmpty() {
					t = PlainRich(url)
				}
				u := url
				t.UpdateMarks(0, t.Len(), func(m *Marks) { m.Link = &u })
				out.Append(t)
				i += used
				continue
			}
		}
		// Bare URLs.
		if autolink && (c == 'h' || c == 'w') && !(hasPrev && (unicode.IsLetter(prev) || unicode.IsDigit(prev))) {
			l := strings.ToLower(rest[:min(8, len(rest))])
			if strings.HasPrefix(l, "https://") || strings.HasPrefix(l, "http://") || strings.HasPrefix(l, "www.") {
				end := len(rest)
				for k, r := range rest {
					if unicode.IsSpace(r) || strings.ContainsRune("<>\"'`", r) || r > 127 {
						end = k
						break
					}
				}
				url := strings.TrimRightFunc(rest[:end], func(r rune) bool {
					return strings.ContainsRune(".,;:!?)]}*_~", r)
				})
				tail := url
				if k := strings.Index(tail, "://"); k >= 0 {
					tail = tail[k+3:]
				}
				if len(tail) > 3 && strings.Contains(tail, ".") {
					flush()
					href := url
					if strings.HasPrefix(l, "www.") {
						href = "https://" + url
					}
					out.Push(url, Marks{Link: &href})
					i += len(url)
					continue
				}
			}
		}
		// Emphasis.
		if c == '*' || c == '_' || c == '~' {
			n := 0
			for n < len(rest) && rune(rest[n]) == c {
				n++
			}
			run := rest[:n]
			var set func(*Marks)
			switch {
			case (c == '*' || c == '_') && n == 3:
				set = func(m *Marks) { m.Bold = true; m.Italic = true }
			case (c == '*' || c == '_') && n == 2:
				set = func(m *Marks) { m.Bold = true }
			case (c == '*' || c == '_') && n == 1:
				set = func(m *Marks) { m.Italic = true }
			case c == '~' && n == 2:
				set = func(m *Marks) { m.Strike = true }
			case c == '~' && n == 1:
				set = func(m *Marks) { m.Underline = true }
			}
			bounded := c == '_' || (c == '~' && n == 1)
			if set != nil && (!bounded || isBoundary(prev, hasPrev)) {
				if end, ok := findRun(rest[n:], run, byte(c), file); ok {
					inner := rest[n : n+end]
					afterIdx := n + end + n
					var after rune
					hasAfter := afterIdx < len(rest)
					if hasAfter {
						after, _ = utf8.DecodeRuneInString(rest[afterIdx:])
					}
					if inner != "" && !unicode.IsSpace(rune(inner[0])) && !unicode.IsSpace(lastRune(inner)) &&
						(!bounded || isBoundary(after, hasAfter)) {
						flush()
						t := mdInlineWith(inner, file, autolink)
						t.UpdateMarks(0, t.Len(), set)
						out.Append(t)
						i += n + end + n
						continue
					}
				}
			}
			plain.WriteString(run)
			i += n
			continue
		}
		plain.WriteRune(c)
		i += size
	}
	flush()
	return out
}

func isASCIIPunct(c byte) bool {
	return (c >= '!' && c <= '/') || (c >= ':' && c <= '@') || (c >= '[' && c <= '`') || (c >= '{' && c <= '~')
}
