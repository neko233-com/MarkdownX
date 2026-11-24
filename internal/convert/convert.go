// Package convert turns documents of other formats into MarkdownX Markdown.
//
// Every reader is pure Go with CGO_ENABLED=0: ZIP and XML come from the
// standard library, so a build needs no C toolchain on any platform.
package convert

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/neko233/markdownx/internal/book"
	"github.com/neko233/markdownx/internal/doc"
)

// Format is a document format a reader understands.
type Format int

const (
	FormatUnknown Format = iota
	FormatMarkdown
	FormatDocx
	FormatXlsx
	FormatEpub
	FormatOfd
	FormatPDF
	FormatRtf
	FormatHTML
	FormatMdxb
)

// String is the format's extension name.
func (f Format) String() string {
	switch f {
	case FormatMarkdown:
		return "markdown"
	case FormatDocx:
		return "docx"
	case FormatXlsx:
		return "xlsx"
	case FormatEpub:
		return "epub"
	case FormatOfd:
		return "ofd"
	case FormatPDF:
		return "pdf"
	case FormatRtf:
		return "rtf"
	case FormatHTML:
		return "html"
	case FormatMdxb:
		return "mdxb"
	default:
		return "unknown"
	}
}

// Detect guesses a format from a file name.
func Detect(name string) Format {
	switch strings.ToLower(filepath.Ext(name)) {
	case ".md", ".markdown", ".mdown", ".mkd", ".txt", ".qmd":
		return FormatMarkdown
	case ".docx":
		return FormatDocx
	case ".xlsx":
		return FormatXlsx
	case ".epub":
		return FormatEpub
	case ".ofd":
		return FormatOfd
	case ".pdf":
		return FormatPDF
	case ".rtf":
		return FormatRtf
	case ".html", ".htm":
		return FormatHTML
	case ".mdxb":
		return FormatMdxb
	}
	return FormatUnknown
}

// Markdown converts data of the named file into Markdown source.
//
// A format this build cannot read returns ErrUnsupported, so the caller can
// show the reason instead of an empty document.
func Markdown(name string, data []byte) (string, error) {
	switch Detect(name) {
	case FormatMarkdown:
		return string(data), nil
	case FormatDocx:
		return Docx(data)
	case FormatXlsx:
		return Xlsx(data)
	case FormatEpub:
		return Epub(data)
	case FormatOfd:
		return Ofd(data)
	case FormatPDF:
		return PDF(data)
	case FormatRtf:
		return Rtf(data)
	case FormatHTML:
		return HTML(data)
	case FormatMdxb:
		return Mdxb(data)
	}
	return "", fmt.Errorf("%w: %s", ErrUnsupported, filepath.Ext(name))
}

// Mdxb flattens a `.mdxb` book into Markdown: one heading per chapter.
func Mdxb(data []byte) (string, error) {
	b, err := book.Parse(data)
	if err != nil {
		return "", err
	}
	dir := "assets"
	out := doc.Export(b.Document(), doc.Options{Assets: &dir})
	return out.Text, nil
}

// ErrUnsupported is returned for a format this build does not read.
var ErrUnsupported = fmt.Errorf("不支持的格式")
