package doc

import (
	"encoding/base64"
	"encoding/json"
	"strings"
)

// rawSummary reads an unmodelled element as plain text plus a link, if any.
func rawSummary(xml string) (string, string) {
	link := ""
	for _, key := range []string{"href", "url", "src", "link"} {
		if v, ok := xmlAttr(xml, key); ok {
			link = v
			break
		}
	}
	return strings.TrimSpace(xmlText(xml)), link
}

// xmlAttr finds the first value of a named attribute in a tag.
func xmlAttr(xml, key string) (string, bool) {
	for at := 0; at < len(xml); at++ {
		lt := strings.IndexByte(xml[at:], '<')
		if lt < 0 {
			return "", false
		}
		start := at + lt
		gt := strings.IndexByte(xml[start:], '>')
		if gt < 0 {
			return "", false
		}
		tag := xml[start+1 : start+gt]
		if strings.HasPrefix(tag, "/") || strings.HasPrefix(tag, "!") || strings.HasPrefix(tag, "?") {
			at = start + gt
			continue
		}
		if v, ok := tagAttr(tag, key); ok {
			return v, true
		}
		at = start + gt
	}
	return "", false
}

func tagAttr(tag, key string) (string, bool) {
	i := 0
	for i < len(tag) {
		for i < len(tag) && (tag[i] == ' ' || tag[i] == '\t' || tag[i] == '\n' || tag[i] == '\r') {
			i++
		}
		start := i
		for i < len(tag) && !strings.ContainsRune(" \t\n\r=", rune(tag[i])) {
			i++
		}
		name := tag[start:i]
		for i < len(tag) && (tag[i] == ' ' || tag[i] == '\t') {
			i++
		}
		if i >= len(tag) || tag[i] != '=' {
			continue
		}
		i++
		for i < len(tag) && (tag[i] == ' ' || tag[i] == '\t') {
			i++
		}
		quote := byte(0)
		if i < len(tag) && (tag[i] == '"' || tag[i] == '\'') {
			quote = tag[i]
			i++
		}
		vstart := i
		if quote != 0 {
			for i < len(tag) && tag[i] != quote {
				i++
			}
		} else {
			for i < len(tag) && !strings.ContainsRune(" \t\n\r", rune(tag[i])) {
				i++
			}
		}
		if strings.EqualFold(name, key) {
			return tag[vstart:i], true
		}
		if quote != 0 && i < len(tag) {
			i++
		}
	}
	return "", false
}

// xmlText is the text content of an element, with <br> as a newline.
func xmlText(xml string) string {
	var b strings.Builder
	for i := 0; i < len(xml); {
		if xml[i] != '<' {
			b.WriteByte(xml[i])
			i++
			continue
		}
		gt := strings.IndexByte(xml[i:], '>')
		if gt < 0 {
			break
		}
		tag := xml[i+1 : i+gt]
		name := tag
		if n := strings.IndexAny(name, " \t\n\r/"); n >= 0 {
			name = name[:n]
		}
		if strings.EqualFold(name, "br") {
			b.WriteByte('\n')
		}
		i += gt + 1
	}
	return b.String()
}

// rewriteHTMLSources rewrites local src/href attributes of an HTML fragment.
func rewriteHTMLSources(src string, f func(string) (string, bool)) string {
	var out strings.Builder
	at := 0
	for at < len(src) {
		i := strings.IndexAny(src[at:], "\"'=")
		if i < 0 {
			out.WriteString(src[at:])
			break
		}
		i += at
		out.WriteString(src[at : i+1])
		at = i + 1
		// Only rewrite values that follow src= or href= on a tag.
		pre := strings.ToLower(src[max(0, i-8):i])
		relevant := strings.HasSuffix(pre, "src=") || strings.HasSuffix(pre, "href=") ||
			strings.HasSuffix(pre, "src =") || strings.HasSuffix(pre, "href =")
		if !relevant || at >= len(src) {
			continue
		}
		quote := src[i]
		if quote != '"' && quote != '\'' {
			continue
		}
		end := strings.IndexByte(src[at:], quote)
		if end < 0 {
			out.WriteString(src[at:])
			break
		}
		value := src[at : at+end]
		if next, ok := f(value); ok {
			out.WriteString(next)
		} else {
			out.WriteString(value)
		}
		out.WriteByte(quote)
		at += end + 1
	}
	return out.String()
}

// MarkdownTableWidget encodes a widget cell without pipes or newlines.
func MarkdownTableWidget(w Interactive) string {
	payload, _ := json.Marshal(map[string]any{
		"kind":   w.Kind.String(),
		"title":  w.Title,
		"height": w.VisibleHeight(),
		"source": w.Source,
	})
	enc := base64.StdEncoding.EncodeToString(payload)
	return `<markdownx-interactive data-base64="` + enc + `"></markdownx-interactive>`
}
