package doc

import (
	"strconv"
	"strings"
)

// inertHTMLTagEnd measures one complete `<tag …>` at the start of s.
func inertHTMLTagEnd(s string) (int, bool) {
	if !strings.HasPrefix(s, "<") {
		return 0, false
	}
	if strings.HasPrefix(s, "<!--") {
		if end := strings.Index(s, "-->"); end >= 0 {
			return end + 3, true
		}
		return 0, false
	}
	gt := strings.IndexByte(s, '>')
	if gt < 0 {
		return 0, false
	}
	return gt + 1, true
}

// inertHTMLRegionEnd measures a comment or a script/style element.
func inertHTMLRegionEnd(s string) (int, bool) {
	if strings.HasPrefix(s, "<!--") {
		if end := strings.Index(s, "-->"); end >= 0 {
			return end + 3, true
		}
		return 0, false
	}
	lower := strings.ToLower(s)
	for _, tag := range []string{"script", "style"} {
		open := "<" + tag
		if !strings.HasPrefix(lower, open) {
			continue
		}
		gt := strings.IndexByte(s, '>')
		if gt < 0 {
			return 0, false
		}
		closeTag := "</" + tag + ">"
		if end := strings.Index(lower[gt:], closeTag); end >= 0 {
			return gt + end + len(closeTag), true
		}
		return len(s), true
	}
	return 0, false
}

func htmlTagName(s string) string {
	t := strings.TrimPrefix(s, "<")
	t = strings.TrimPrefix(t, "/")
	end := len(t)
	for i := 0; i < len(t); i++ {
		c := t[i]
		if c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '>' || c == '/' {
			end = i
			break
		}
	}
	return strings.ToLower(t[:end])
}

// htmlImageAt parses an `<img …>` tag at the start of s.
func htmlImageAt(s string) (img InlineImage, width, height *float32, used int, ok bool) {
	used, ok = inertHTMLTagEnd(s)
	if !ok || htmlTagName(s) != "img" {
		return InlineImage{}, nil, nil, 0, false
	}
	tag := s[:used]
	src, hasSrc := tagAttr(tag, "src")
	if !hasSrc && !strings.Contains(tag, "data:") {
		// An srcset-only image still has something to show.
		if alt, ok := tagAttr(tag, "alt"); !ok || alt == "" {
			return InlineImage{}, nil, nil, 0, false
		}
	}
	alt, _ := tagAttr(tag, "alt")
	img = InlineImage{SrcKind: "src", Src: unescapeAttr(src), Alt: unescapeAttr(alt)}
	width = cssLength(tag, "width")
	height = cssLength(tag, "height")
	return img, width, height, used, true
}

func unescapeAttr(s string) string {
	r := strings.NewReplacer("&amp;", "&", "&lt;", "<", "&gt;", ">", "&quot;", `"`, "&#39;", "'")
	return r.Replace(s)
}

func cssLength(tag, name string) *float32 {
	if v, ok := tagAttr(tag, name); ok {
		if n, err := strconv.ParseFloat(strings.TrimSuffix(strings.TrimSpace(v), "px"), 32); err == nil && n > 0 {
			f := float32(n)
			return &f
		}
	}
	if style, ok := tagAttr(tag, "style"); ok {
		for _, part := range strings.Split(style, ";") {
			key, value, found := strings.Cut(part, ":")
			if !found || !strings.EqualFold(strings.TrimSpace(key), name) {
				continue
			}
			if n, err := strconv.ParseFloat(strings.TrimSuffix(strings.TrimSpace(value), "px"), 32); err == nil && n > 0 {
				f := float32(n)
				return &f
			}
		}
	}
	return nil
}

// htmlImageLine parses a line that holds only an `<img>` tag.
func htmlImageLine(s string) (img InlineImage, width, height *float32, ok bool) {
	s = strings.TrimSpace(s)
	if !strings.HasPrefix(s, "<") || htmlTagName(s) != "img" {
		return InlineImage{}, nil, nil, false
	}
	used, ok := inertHTMLTagEnd(s)
	if !ok {
		return InlineImage{}, nil, nil, false
	}
	tail := strings.TrimSpace(s[used:])
	if tail != "" && tail != "</img>" {
		return InlineImage{}, nil, nil, false
	}
	img, w, h, _, ok := htmlImageAt(s)
	return img, w, h, ok
}

// htmlImageBlockAt parses an `<img>` or `<figure><img></figure>` block.
func htmlImageBlockAt(lines []string) (img InlineImage, width, height *float32, used int, ok bool) {
	if len(lines) == 0 {
		return InlineImage{}, nil, nil, 0, false
	}
	if img, w, h, ok := htmlImageLine(lines[0]); ok {
		return img, w, h, 1, true
	}
	src := strings.Join(lines, "\n")
	if htmlTagName(src) != "figure" {
		return InlineImage{}, nil, nil, 0, false
	}
	end := balancedElementEnd(src, "figure")
	if end <= 0 {
		return InlineImage{}, nil, nil, 0, false
	}
	fragment := src[:end]
	at := indexFold(fragment, "<img")
	if at < 0 {
		return InlineImage{}, nil, nil, 0, false
	}
	img, w, h, _, good := htmlImageAt(fragment[at:])
	if !good {
		return InlineImage{}, nil, nil, 0, false
	}
	used = strings.Count(strings.TrimRight(fragment, "\n"), "\n") + 1
	return img, w, h, used, true
}

func indexFold(s, needle string) int {
	return strings.Index(strings.ToLower(s), strings.ToLower(needle))
}

// balancedElementEnd measures a balanced element of the given tag.
func balancedElementEnd(s, tag string) int {
	lower := strings.ToLower(s)
	open := "<" + tag
	closeTag := "</" + tag + ">"
	depth := 0
	at := 0
	for at < len(lower) {
		o := strings.Index(lower[at:], open)
		c := strings.Index(lower[at:], closeTag)
		switch {
		case o < 0 && c < 0:
			return 0
		case c < 0 || (o >= 0 && o < c):
			depth++
			at += o + len(open)
		default:
			depth--
			at += c + len(closeTag)
			if depth == 0 {
				return at
			}
		}
	}
	return 0
}

// htmlMediaBlock matches a run of lines holding a media element.
func htmlMediaBlock(lines []string) (string, int, bool) {
	if len(lines) == 0 {
		return "", 0, false
	}
	first := strings.TrimSpace(lines[0])
	if !strings.HasPrefix(first, "<") {
		return "", 0, false
	}
	tag := htmlTagName(first)
	switch tag {
	case "video", "audio", "iframe", "picture", "img", "source":
	default:
		if tag != "figure" && tag != "div" && tag != "table" {
			return "", 0, false
		}
	}
	joined := strings.Join(lines, "\n")
	end := balancedElementEnd(joined, tag)
	if end <= 0 {
		// A self-closing or void media tag is one line.
		if used, ok := inertHTMLTagEnd(first); ok && used == len(first) {
			if tag == "video" || tag == "audio" || tag == "iframe" || tag == "img" || tag == "picture" {
				if hasMediaMarker(first) {
					return first, 1, true
				}
			}
		}
		return "", 0, false
	}
	fragment := joined[:end]
	if !hasMediaMarker(fragment) {
		return "", 0, false
	}
	used := strings.Count(strings.TrimRight(fragment, "\n"), "\n") + 1
	return fragment, used, true
}

func hasMediaMarker(s string) bool {
	l := strings.ToLower(s)
	for _, marker := range []string{"<video", "<audio", "<iframe", "<img", "<picture", "<source"} {
		if strings.Contains(l, marker) {
			return true
		}
	}
	return false
}
