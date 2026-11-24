package convert

import (
	"fmt"
	"strings"
)

// Rtf turns an RTF document into Markdown by keeping its text, its paragraph
// breaks and its bold, italic and underline runs.
func Rtf(data []byte) (string, error) {
	src := string(data)
	if !strings.HasPrefix(strings.TrimSpace(src), "{") {
		return "", fmt.Errorf("rtf: 不是 RTF 文件")
	}
	var b strings.Builder
	// A stack of group states; \b, \i, \u and \ul are per group.
	type state struct {
		bold, italic, underline bool
	}
	stack := []state{{}}
	top := func() *state { return &stack[len(stack)-1] }

	skipGroup := 0
	i := 0
	for i < len(src) {
		c := src[i]
		switch c {
		case '{':
			stack = append(stack, *top())
			i++
		case '}':
			if len(stack) > 1 {
				stack = stack[:len(stack)-1]
			}
			i++
		case '\\':
			// Control word or symbol.
			if i+1 >= len(src) {
				i++
				continue
			}
			if src[i+1] == '\'' {
				// \'hh: a byte in the document's code page.
				if i+3 < len(src) {
					if v, ok := hexByte(src[i+2], src[i+3]); ok {
						if skipGroup == 0 {
							b.WriteByte(v)
						}
					}
					i += 4
					continue
				}
				i++
				continue
			}
			if src[i+1] == '\\' || src[i+1] == '{' || src[i+1] == '}' {
				if skipGroup == 0 {
					b.WriteByte(src[i+1])
				}
				i += 2
				continue
			}
			if src[i+1] == '*' {
				// An ignorable destination.
				skipGroup++
				i += 2
				continue
			}
			// A word: letters then optional number.
			j := i + 1
			for j < len(src) && isAlpha(src[j]) {
				j++
			}
			word := src[i+1 : j]
			numStart := j
			for j < len(src) && (src[j] == '-' || (src[j] >= '0' && src[j] <= '9')) {
				j++
			}
			num := src[numStart:j]
			if j < len(src) && src[j] == ' ' {
				j++
			}
			i = j
			switch word {
			case "fonttbl", "colortbl", "stylesheet", "info", "pict", "object",
				"header", "footer", "headerl", "headerr", "footerl", "footerr",
				"footnote", "field", "fldinst", "shppict", "nonshppict":
				skipGroup++
			case "par", "line":
				if skipGroup == 0 {
					b.WriteString("\n\n")
				}
			case "tab":
				if skipGroup == 0 {
					b.WriteString("\t")
				}
			case "b":
				if skipGroup == 0 {
					on := num != "0"
					if on != top().bold {
						b.WriteString("**")
						top().bold = on
					}
				}
			case "i":
				if skipGroup == 0 {
					on := num != "0"
					if on != top().italic {
						b.WriteString("*")
						top().italic = on
					}
				}
			case "ul":
				if skipGroup == 0 {
					on := num != "0"
					if on != top().underline {
						if on {
							b.WriteString("<u>")
						} else {
							b.WriteString("</u>")
						}
						top().underline = on
					}
				}
			case "u":
				if skipGroup == 0 && num != "" {
					if cp, err := parseInt(num); err == nil {
						if cp < 0 {
							cp += 65536
						}
						b.WriteRune(rune(cp))
					}
					// \uN is followed by an ANSI fallback character, usually `?`.
					if i < len(src) && src[i] == '?' {
						i++
					}
				}
			}
		case '\r', '\n':
			i++
		default:
			if skipGroup == 0 {
				b.WriteByte(c)
			}
			i++
		}
		if skipGroup > 0 && c == '}' {
			skipGroup--
		}
	}
	out := strings.TrimSpace(b.String())
	if out == "" {
		return "", fmt.Errorf("rtf: 正文为空")
	}
	return out + "\n", nil
}

func isAlpha(c byte) bool {
	return c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z'
}

func hexByte(h, l byte) (byte, bool) {
	hi, ok1 := hexVal(h)
	lo, ok2 := hexVal(l)
	if !ok1 || !ok2 {
		return 0, false
	}
	return hi<<4 | lo, true
}

func hexVal(c byte) (byte, bool) {
	switch {
	case c >= '0' && c <= '9':
		return c - '0', true
	case c >= 'a' && c <= 'f':
		return c - 'a' + 10, true
	case c >= 'A' && c <= 'F':
		return c - 'A' + 10, true
	}
	return 0, false
}
