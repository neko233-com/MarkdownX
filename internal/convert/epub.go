package convert

import (
	"archive/zip"
	"bytes"
	"encoding/xml"
	"fmt"
	"io"
	"path"
	"strings"
)

// Epub reads an EPUB ebook: its spine's XHTML documents, in reading order,
// become one Markdown document.
func Epub(data []byte) (string, error) {
	zr, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		return "", fmt.Errorf("epub: %w", err)
	}
	files := map[string]*zip.File{}
	for _, f := range zr.File {
		files[path.Clean(f.Name)] = f
	}

	rootfile := ""
	if f := files["META-INF/container.xml"]; f != nil {
		var c struct {
			Rootfiles []struct {
				FullPath string `xml:"full-path,attr"`
			} `xml:"rootfiles>rootfile"`
		}
		if err := decodeZipXML(f, &c); err == nil && len(c.Rootfiles) > 0 {
			rootfile = path.Clean(c.Rootfiles[0].FullPath)
		}
	}
	if rootfile == "" {
		for name := range files {
			if strings.HasSuffix(name, ".opf") {
				rootfile = name
				break
			}
		}
	}
	if rootfile == "" {
		return "", fmt.Errorf("epub: 找不到 OPF 清单")
	}

	var opf struct {
		Manifest []struct {
			ID   string `xml:"id,attr"`
			Href string `xml:"href,attr"`
			Type string `xml:"media-type,attr"`
		} `xml:"manifest>item"`
		Spine []struct {
			IDRef string `xml:"idref,attr"`
		} `xml:"spine>itemref"`
	}
	if err := decodeZipXML(files[rootfile], &opf); err != nil {
		return "", fmt.Errorf("epub: %w", err)
	}
	base := path.Dir(rootfile)
	byID := map[string]string{}
	for _, it := range opf.Manifest {
		byID[it.ID] = it.Href
	}
	var out strings.Builder
	for _, ref := range opf.Spine {
		href, ok := byID[ref.IDRef]
		if !ok {
			continue
		}
		name := path.Clean(path.Join(base, href))
		f := files[name]
		if f == nil {
			f = files[path.Clean(href)]
		}
		if f == nil {
			continue
		}
		raw, err := readZipFile(f)
		if err != nil {
			continue
		}
		md := htmlToMarkdown(string(raw))
		if md == "" {
			continue
		}
		out.WriteString(md)
		out.WriteString("\n\n")
	}
	if out.Len() == 0 {
		return "", fmt.Errorf("epub: 正文为空")
	}
	return strings.TrimSpace(out.String()) + "\n", nil
}

func decodeZipXML(f *zip.File, v any) error {
	raw, err := readZipFile(f)
	if err != nil {
		return err
	}
	dec := xml.NewDecoder(bytes.NewReader(raw))
	dec.CharsetReader = charsetReader
	return dec.Decode(v)
}

func readZipFile(f *zip.File) ([]byte, error) {
	rc, err := f.Open()
	if err != nil {
		return nil, err
	}
	defer rc.Close()
	return io.ReadAll(io.LimitReader(rc, 1<<28))
}

// charsetReader passes the bytes through: an OFD or EPUB manifest is UTF-8,
// and any other encoding still reads as its own bytes.
func charsetReader(_ string, input io.Reader) (io.Reader, error) { return input, nil }
