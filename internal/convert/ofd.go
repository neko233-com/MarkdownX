package convert

import (
	"archive/zip"
	"bytes"
	"encoding/xml"
	"fmt"
	"path"
	"sort"
	"strings"
)

// Ofd reads a Chinese OFD layout document: every page's text objects, in page
// order, become paragraphs.
func Ofd(data []byte) (string, error) {
	zr, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		return "", fmt.Errorf("ofd: %w", err)
	}
	files := map[string]*zip.File{}
	var pageNames []string
	for _, f := range zr.File {
		name := path.Clean(f.Name)
		files[name] = f
		lower := strings.ToLower(name)
		if strings.HasPrefix(lower, "doc_0/pages/") && strings.HasSuffix(lower, "/content.xml") {
			pageNames = append(pageNames, name)
		}
	}
	if len(pageNames) == 0 {
		return "", fmt.Errorf("ofd: 找不到页面")
	}
	sort.Strings(pageNames)

	var out strings.Builder
	for _, name := range pageNames {
		raw, err := readZipFile(files[name])
		if err != nil {
			continue
		}
		text := ofdPageText(raw)
		if strings.TrimSpace(text) == "" {
			continue
		}
		out.WriteString(strings.TrimSpace(text))
		out.WriteString("\n\n")
	}
	if out.Len() == 0 {
		return "", fmt.Errorf("ofd: 正文为空")
	}
	return strings.TrimSpace(out.String()) + "\n", nil
}

// ofdPageText collects the TextCode elements of one page.
func ofdPageText(raw []byte) string {
	dec := xml.NewDecoder(bytes.NewReader(raw))
	dec.CharsetReader = charsetReader
	var lines []string
	var cur strings.Builder
	depth := 0
	for {
		tok, err := dec.Token()
		if err != nil {
			break
		}
		switch t := tok.(type) {
		case xml.StartElement:
			switch strings.ToLower(t.Name.Local) {
			case "textcode":
				depth++
				for _, a := range t.Attr {
					if strings.EqualFold(a.Name.Local, "x") {
						// A new object starts a new line when it moves left.
						if cur.Len() > 0 {
							lines = append(lines, cur.String())
							cur.Reset()
						}
					}
				}
			}
		case xml.EndElement:
			if strings.EqualFold(t.Name.Local, "textcode") {
				depth--
				if depth <= 0 && cur.Len() > 0 {
					lines = append(lines, cur.String())
					cur.Reset()
				}
			}
		case xml.CharData:
			if depth > 0 {
				cur.Write(t)
			}
		}
	}
	if cur.Len() > 0 {
		lines = append(lines, cur.String())
	}
	var kept []string
	for _, l := range lines {
		l = strings.TrimSpace(l)
		if l != "" {
			kept = append(kept, l)
		}
	}
	return strings.Join(kept, "\n\n")
}
