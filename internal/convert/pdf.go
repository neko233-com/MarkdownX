package convert

import (
	"bytes"
	"compress/zlib"
	"fmt"
	"io"
	"strconv"
	"strings"
)

// PDF extracts the text of a PDF file.
//
// It reads the page content streams, inflating FlateDecode filters, and keeps
// the strings a page draws with Tj, TJ and ' operators. That covers the text
// of PDFs produced by word processors and browsers; a scanned page has no text
// to read and comes back empty.
func PDF(data []byte) (string, error) {
	if !bytes.HasPrefix(bytes.TrimSpace(data), []byte("%PDF")) {
		return "", fmt.Errorf("pdf: 不是 PDF 文件")
	}
	var out strings.Builder
	for _, stream := range pdfStreams(data) {
		text := pdfText(stream)
		if strings.TrimSpace(text) == "" {
			continue
		}
		out.WriteString(text)
		out.WriteString("\n\n")
	}
	if out.Len() == 0 {
		return "", fmt.Errorf("pdf: 没有可提取的文本（可能是扫描件）")
	}
	return strings.TrimSpace(out.String()) + "\n", nil
}

// pdfStreams yields every stream's decoded bytes.
func pdfStreams(data []byte) [][]byte {
	var out [][]byte
	at := 0
	for {
		i := bytes.Index(data[at:], []byte("stream"))
		if i < 0 {
			return out
		}
		start := at + i + len("stream")
		if start < len(data) && data[start] == '\r' {
			start++
		}
		if start < len(data) && data[start] == '\n' {
			start++
		}
		end := bytes.Index(data[start:], []byte("endstream"))
		if end < 0 {
			return out
		}
		raw := data[start : start+end]
		if decoded, err := inflate(raw); err == nil {
			out = append(out, decoded)
		} else {
			out = append(out, raw)
		}
		at = start + end + len("endstream")
	}
}

func inflate(raw []byte) ([]byte, error) {
	zr, err := zlib.NewReader(bytes.NewReader(bytes.TrimSpace(raw)))
	if err != nil {
		return nil, err
	}
	defer zr.Close()
	return io.ReadAll(io.LimitReader(zr, 1<<28))
}

// pdfText turns one content stream into text.
func pdfText(stream []byte) string {
	var out strings.Builder
	var line strings.Builder
	flushLine := func() {
		s := strings.TrimRight(line.String(), " ")
		if s != "" {
			out.WriteString(s)
			out.WriteByte('\n')
		}
		line.Reset()
	}
	i := 0
	for i < len(stream) {
		c := stream[i]
		switch c {
		case '(':
			s, next := pdfString(stream, i+1)
			line.WriteString(s)
			i = next
			continue
		case '<':
			if i+1 < len(stream) && stream[i+1] == '<' {
				// A dictionary inside the content: skip it.
				i = skipDict(stream, i)
				continue
			}
			s, next := pdfHexString(stream, i+1)
			line.WriteString(s)
			i = next
			continue
		case 'T', 't', '\'', '"':
			// Operator keywords.
			if word, next := pdfWord(stream, i); word != "" {
				switch word {
				case "Tj", "TJ", "'", "\"":
					// The string was already taken above.
				case "Td", "TD", "T*", "Tm", "ET":
					flushLine()
				}
				i = next
				continue
			}
		}
		i++
	}
	flushLine()
	return strings.Join(strings.Split(out.String(), "\n"), "\n")
}

func skipDict(s []byte, at int) int {
	depth := 0
	for i := at; i < len(s); i++ {
		switch s[i] {
		case '<':
			if i+1 < len(s) && s[i+1] == '<' {
				depth++
				i++
			}
		case '>':
			if i+1 < len(s) && s[i+1] == '>' {
				depth--
				i++
				if depth == 0 {
					return i + 1
				}
			}
		}
	}
	return len(s)
}

// pdfString reads a literal `(...)` string, handling escapes and nesting.
func pdfString(s []byte, at int) (string, int) {
	var b strings.Builder
	depth := 1
	for i := at; i < len(s); i++ {
		c := s[i]
		switch c {
		case '\\':
			if i+1 >= len(s) {
				return b.String(), i + 1
			}
			i++
			switch s[i] {
			case 'n':
				b.WriteByte('\n')
			case 'r':
				b.WriteByte('\r')
			case 't':
				b.WriteByte('\t')
			case 'b':
				b.WriteByte('\b')
			case 'f':
				b.WriteByte('\f')
			case '(', ')', '\\':
				b.WriteByte(s[i])
			default:
				if s[i] >= '0' && s[i] <= '7' {
					v := 0
					n := 0
					for n < 3 && i < len(s) && s[i] >= '0' && s[i] <= '7' {
						v = v*8 + int(s[i]-'0')
						i++
						n++
					}
					i--
					b.WriteByte(byte(v))
				} else {
					b.WriteByte(s[i])
				}
			}
		case '(':
			depth++
			b.WriteByte(c)
		case ')':
			depth--
			if depth == 0 {
				return b.String(), i + 1
			}
			b.WriteByte(c)
		default:
			b.WriteByte(c)
		}
	}
	return b.String(), len(s)
}

// pdfHexString reads a `<...>` hex string.
func pdfHexString(s []byte, at int) (string, int) {
	var hex strings.Builder
	for i := at; i < len(s); i++ {
		c := s[i]
		if c == '>' {
			return hexToString(hex.String()), i + 1
		}
		if isHexByte(c) {
			hex.WriteByte(c)
		}
	}
	return hexToString(hex.String()), len(s)
}

func isHexByte(c byte) bool {
	return c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F'
}

func hexToString(h string) string {
	if len(h)%2 == 1 {
		h += "0"
	}
	var b strings.Builder
	for i := 0; i+1 < len(h); i += 2 {
		v, err := strconv.ParseUint(h[i:i+2], 16, 8)
		if err != nil {
			continue
		}
		b.WriteByte(byte(v))
	}
	return b.String()
}

// pdfWord reads an alphabetic operator keyword.
func pdfWord(s []byte, at int) (string, int) {
	i := at
	for i < len(s) && (s[i] >= 'a' && s[i] <= 'z' || s[i] >= 'A' && s[i] <= 'Z' || s[i] == '*' || s[i] == '\'' || s[i] == '"') {
		i++
	}
	return string(s[at:i]), i
}
