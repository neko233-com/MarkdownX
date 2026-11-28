// Package export writes a document as a self-contained HTML page.
package export

import (
	"encoding/base64"
	"fmt"
	"html"
	"strings"

	"github.com/egoist/mygo/ui/markdown"
	"github.com/neko233/markdownx/internal/diagram"
	"github.com/neko233/markdownx/internal/doc"
)

// Options are the page's settings.
type Options struct {
	// Title is the page title and the document heading.
	Title string
	// CSS replaces the built-in stylesheet.
	CSS string
	// Diagrams writes fenced diagrams as embedded SVG images.
	Diagrams bool
}

const baseCSS = `
:root{color-scheme:light dark;--fg:#1f2328;--bg:#ffffff;--muted:#6b7280;--line:#e5e7eb;--code:#f6f8fa;--accent:#4e83fd}
@media (prefers-color-scheme:dark){:root{--fg:#e6edf3;--bg:#0d1117;--muted:#8b949e;--line:#30363d;--code:#161b22;--accent:#6ea8fe}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.75 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Hiragino Sans GB","Microsoft YaHei",sans-serif}
main{max-width:760px;margin:0 auto;padding:48px 24px 96px}
h1,h2,h3,h4,h5,h6{line-height:1.3;margin:1.6em 0 .6em;font-weight:650}
h1{font-size:2em}h2{font-size:1.5em}h3{font-size:1.25em}h4{font-size:1.1em}
p{margin:0 0 1em}
a{color:var(--accent);text-decoration:none}
a:hover{text-decoration:underline}
code{background:var(--code);padding:.15em .35em;border-radius:4px;font-size:.9em;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
pre{background:var(--code);padding:14px 16px;border-radius:8px;overflow:auto}
pre code{background:none;padding:0}
blockquote{margin:0 0 1em;padding:.2em 1em;border-left:3px solid var(--line);color:var(--muted)}
hr{border:0;border-top:1px solid var(--line);margin:2em 0}
ul,ol{padding-left:1.6em;margin:0 0 1em}
li{margin:.25em 0}
table{border-collapse:collapse;width:100%;margin:0 0 1em;font-size:.95em}
th,td{border:1px solid var(--line);padding:6px 10px;text-align:left;vertical-align:top}
th{background:var(--code);font-weight:600}
img{max-width:100%}
.diagram{margin:1.25em 0;text-align:center}
.diagram img{height:auto}
.toc ul{margin:.15em 0;padding-left:1.3em}
.toc{margin:0 0 1.5em;padding-left:.5em}
.footnotes{margin-top:2rem;border-top:1px solid var(--line);padding-top:.8rem;font-size:.9em}
.footnote-backlinks{margin-bottom:1em}
del{color:var(--muted)}
`

// Page renders the whole document as one HTML file.
func Page(d *doc.Document, opts Options) string {
	var b strings.Builder
	b.WriteString("<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n")
	b.WriteString("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n")
	title := opts.Title
	if title == "" {
		title = strings.TrimSpace(d.Title.PlainText())
	}
	if title != "" {
		b.WriteString("<title>" + html.EscapeString(title) + "</title>\n")
	}
	css := baseCSS
	if opts.CSS != "" {
		css = opts.CSS
	}
	b.WriteString("<style>" + css + "</style>\n</head>\n<body>\n<main>\n")
	writeDocument(&b, d, d, opts.Diagrams)
	b.WriteString("</main>\n</body>\n</html>\n")
	return b.String()
}

// Fragment renders document content for rich clipboard formats.
func Fragment(d *doc.Document) string {
	return FragmentWithDefinitions(d, d)
}

// FragmentWithDefinitions resolves links and notes outside the fragment.
func FragmentWithDefinitions(d, definitions *doc.Document) string {
	var b strings.Builder
	writeDocument(&b, d, definitions, false)
	return b.String()
}

func writeDocument(b *strings.Builder, d, definitions *doc.Document, diagrams bool) {
	w := documentHTML{
		Builder: b, diagrams: diagrams, headings: d.Headings(markdown.Slug), refs: d.ReferencesFrom(definitions),
		anchors: make(map[doc.BlockID]string), used: make(map[string]bool), notes: make(map[doc.BlockID]*htmlFootnote),
	}
	for _, h := range w.headings {
		w.anchors[h.Block] = h.Slug
		w.used[h.Slug] = true
	}
	if !d.Title.IsEmpty() {
		w.heading(0, 1, d.Title)
	}
	for _, blk := range d.Blocks {
		w.block(blk)
	}
	w.footnotes()
}

func (w *documentHTML) block(blk *doc.Block) {
	b := w.Builder
	switch k := blk.Kind.(type) {
	case doc.TextKind:
		w.paragraph(blk.Text)
	case doc.HeadingKind:
		w.heading(blk.ID, min(6, max(1, k.Level)), blk.Text)
	case doc.BulletKind:
		w.list("ul", blk)
	case doc.OrderedKind:
		w.list("ol", blk)
	case doc.TodoKind:
		mark := "☐"
		if k.Done {
			mark = "☑"
		}
		fmt.Fprintf(b, "<ul><li>%s %s</li></ul>\n", mark, w.inline(blk.Text, true))
		w.children(blk)
	case doc.CodeKind:
		lang := k.Lang
		if w.diagrams && diagram.Language(lang) != "" && writeDiagram(b, lang, blk.Text.Text) {
			return
		}
		if strings.EqualFold(lang, "plain text") {
			lang = ""
		}
		if lang != "" {
			fmt.Fprintf(b, "<pre><code class=\"language-%s\">", html.EscapeString(lang))
		} else {
			b.WriteString("<pre><code>")
		}
		b.WriteString(html.EscapeString(blk.Text.Text))
		b.WriteString("</code></pre>\n")
	case doc.QuoteKind:
		b.WriteString("<blockquote>\n")
		w.children(blk)
		b.WriteString("</blockquote>\n")
	case doc.CalloutKind:
		b.WriteString("<blockquote>\n<p>" + html.EscapeString(k.Emoji) + "</p>\n")
		w.children(blk)
		b.WriteString("</blockquote>\n")
	case doc.DividerKind:
		b.WriteString("<hr>\n")
	case doc.TOCKind:
		w.contents()
	case doc.DefinitionKind:
		if doc.DefinitionBlock(blk.Text.Text) == nil {
			w.paragraph(blk.Text)
		}
		w.children(blk)
	case doc.ImageKind:
		if k.Src != "" {
			alt := ""
			if k.Caption != nil {
				alt = *k.Caption
			}
			fmt.Fprintf(b, "<p><img src=\"%s\" alt=\"%s\"></p>\n",
				html.EscapeString(k.Src), html.EscapeString(alt))
		}
	case doc.TableKind:
		w.table(k.Table)
	case doc.GridKind, doc.ColumnKind:
		w.children(blk)
	case doc.FileKind:
		fmt.Fprintf(b, "<p>📎 <a href=\"%s\">%s</a></p>\n",
			html.EscapeString(k.File.Src), html.EscapeString(k.File.Name))
	case doc.SubPageListKind, doc.CountdownKind, doc.InteractiveKindBlock:
		// Nothing portable to render outside the app.
	case doc.RawKind:
		b.WriteString(html.EscapeString(k.XML) + "\n")
	}
}

func writeDiagram(b *strings.Builder, language, source string) bool {
	svg, err := diagram.Render(language, source, "sans-serif", false)
	if err != nil {
		return false
	}
	const dataURL = "data:image/svg+xml;base64,"
	b.WriteString("<figure class=\"diagram\"><picture>")
	if dark, err := diagram.Render(language, source, "sans-serif", true); err == nil {
		fmt.Fprintf(b, "<source media=\"(prefers-color-scheme: dark)\" srcset=\"%s%s\">", dataURL, base64.StdEncoding.EncodeToString(dark))
	}
	fmt.Fprintf(b, "<img src=\"%s%s\" alt=\"%s 图表\"></picture></figure>\n", dataURL, base64.StdEncoding.EncodeToString(svg), diagram.Language(language))
	return true
}

func (w *documentHTML) children(blk *doc.Block) {
	for _, c := range blk.Children {
		w.block(c)
	}
}

func (w *documentHTML) paragraph(t doc.RichText) {
	s := w.inline(t, true)
	if strings.TrimSpace(s) == "" {
		return
	}
	w.WriteString("<p>" + s + "</p>\n")
}

func (w *documentHTML) list(tag string, blk *doc.Block) {
	fmt.Fprintf(w, "<%s><li>%s", tag, w.inline(blk.Text, true))
	if len(blk.Children) > 0 {
		w.WriteByte('\n')
		w.children(blk)
	}
	w.WriteString("</li></" + tag + ">\n")
}

func (w *documentHTML) table(t *doc.Table) {
	b := w.Builder
	if t == nil || len(t.Rows) == 0 {
		return
	}
	b.WriteString("<table>\n")
	for r, row := range t.Rows {
		b.WriteString("<tr>")
		for c := range row {
			cell := row[c]
			if cell.IsCovered() {
				continue
			}
			tag := "td"
			if r < t.HeaderRows {
				tag = "th"
			}
			var inner []string
			for _, cb := range cell.Blocks {
				if cb.Text.Text != "" {
					inner = append(inner, w.inline(cb.Text, true))
				}
			}
			fmt.Fprintf(b, "<%s>%s</%s>", tag, strings.Join(inner, "<br>"), tag)
		}
		b.WriteString("</tr>\n")
	}
	b.WriteString("</table>\n")
}

// inline renders one rich-text line, including references from the document.
func (w *documentHTML) inline(t doc.RichText, links bool) string {
	var b strings.Builder
	for _, sp := range t.Spans() {
		src := t.Text[sp.Start:sp.End]
		var text string
		switch {
		case isReference(sp.Marks.Inline):
			ref, _ := sp.Marks.Inline.Reference()
			var rendered strings.Builder
			for range src {
				rendered.WriteString(w.reference(ref, links))
			}
			text = rendered.String()
		case sp.Marks.Inline != nil && isEquation(sp.Marks.Inline):
			src, _ := sp.Marks.Inline.Equation()
			text = html.EscapeString("$" + src + "$")
		case sp.Marks.Inline != nil && isImage(sp.Marks.Inline):
			img, _ := sp.Marks.Inline.Image()
			text = fmt.Sprintf("<img src=\"%s\" alt=\"%s\">",
				html.EscapeString(img.Src), html.EscapeString(img.Alt))
		case sp.Marks.Inline != nil && isDocLink(sp.Marks.Inline):
			l, _ := sp.Marks.Inline.DocLink()
			text = html.EscapeString(l.Title)
		case sp.Marks.Inline != nil:
			// Other inline objects export as their text.
			text = html.EscapeString(strings.ReplaceAll(src, doc.Obj, ""))
		default:
			text = html.EscapeString(src)
		}
		if sp.Marks.Code {
			text = "<code>" + text + "</code>"
		}
		if sp.Marks.Underline {
			text = "<u>" + text + "</u>"
		}
		if sp.Marks.Strike {
			text = "<del>" + text + "</del>"
		}
		if sp.Marks.Italic {
			text = "<em>" + text + "</em>"
		}
		if sp.Marks.Bold {
			text = "<strong>" + text + "</strong>"
		}
		if links && sp.Marks.Link != nil && sp.Marks.Inline == nil {
			text = htmlLink(text, *sp.Marks.Link, "")
		}
		b.WriteString(text)
	}
	// A paragraph's own newlines are hard breaks.
	return strings.ReplaceAll(b.String(), "\n", "<br>")
}

func isReference(in *doc.Inline) bool {
	_, ok := in.Reference()
	return ok
}

func isEquation(in *doc.Inline) bool {
	_, ok := in.Equation()
	return ok
}

func isImage(in *doc.Inline) bool {
	_, ok := in.Image()
	return ok
}

func isDocLink(in *doc.Inline) bool {
	_, ok := in.DocLink()
	return ok
}
