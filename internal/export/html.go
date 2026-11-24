// Package export writes a document as a self-contained HTML page.
package export

import (
	"fmt"
	"html"
	"strings"

	"github.com/neko233/markdownx/internal/doc"
)

// Options are the page's settings.
type Options struct {
	// Title is the page title and the document heading.
	Title string
	// CSS replaces the built-in stylesheet.
	CSS string
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
	if t := strings.TrimSpace(d.Title.Text); t != "" {
		b.WriteString("<h1>" + html.EscapeString(t) + "</h1>\n")
	}
	for _, blk := range d.Blocks {
		writeBlock(&b, blk)
	}
	b.WriteString("</main>\n</body>\n</html>\n")
	return b.String()
}

func writeBlock(b *strings.Builder, blk *doc.Block) {
	switch k := blk.Kind.(type) {
	case doc.TextKind:
		writeParagraph(b, blk.Text)
	case doc.HeadingKind:
		lvl := int(k.Level)
		if lvl < 1 {
			lvl = 1
		}
		if lvl > 6 {
			lvl = 6
		}
		fmt.Fprintf(b, "<h%d>%s</h%d>\n", lvl, inlineHTML(blk.Text), lvl)
	case doc.BulletKind:
		writeList(b, "ul", blk)
	case doc.OrderedKind:
		writeList(b, "ol", blk)
	case doc.TodoKind:
		mark := "☐"
		if k.Done {
			mark = "☑"
		}
		fmt.Fprintf(b, "<ul><li>%s %s</li></ul>\n", mark, inlineHTML(blk.Text))
		writeChildren(b, blk)
	case doc.CodeKind:
		lang := k.Lang
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
		writeChildren(b, blk)
		b.WriteString("</blockquote>\n")
	case doc.CalloutKind:
		b.WriteString("<blockquote>\n<p>" + html.EscapeString(k.Emoji) + "</p>\n")
		writeChildren(b, blk)
		b.WriteString("</blockquote>\n")
	case doc.DividerKind:
		b.WriteString("<hr>\n")
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
		writeTable(b, k.Table)
	case doc.GridKind, doc.ColumnKind:
		writeChildren(b, blk)
	case doc.FileKind:
		fmt.Fprintf(b, "<p>📎 <a href=\"%s\">%s</a></p>\n",
			html.EscapeString(k.File.Src), html.EscapeString(k.File.Name))
	case doc.SubPageListKind, doc.CountdownKind, doc.InteractiveKindBlock:
		// Nothing portable to render outside the app.
	case doc.RawKind:
		b.WriteString(html.EscapeString(k.XML) + "\n")
	}
}

func writeChildren(b *strings.Builder, blk *doc.Block) {
	for _, c := range blk.Children {
		writeBlock(b, c)
	}
}

func writeParagraph(b *strings.Builder, t doc.RichText) {
	s := inlineHTML(t)
	if strings.TrimSpace(s) == "" {
		return
	}
	b.WriteString("<p>" + s + "</p>\n")
}

func writeList(b *strings.Builder, tag string, blk *doc.Block) {
	fmt.Fprintf(b, "<%s><li>%s", tag, inlineHTML(blk.Text))
	if len(blk.Children) > 0 {
		b.WriteString("\n")
		for _, c := range blk.Children {
			if c.Kind.IsList() {
				writeBlock(b, c)
				continue
			}
			fmt.Fprintf(b, "<p>%s</p>\n", inlineHTML(c.Text))
		}
	}
	b.WriteString("</li></" + tag + ">\n")
}

func writeTable(b *strings.Builder, t *doc.Table) {
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
					inner = append(inner, inlineHTML(cb.Text))
				}
			}
			fmt.Fprintf(b, "<%s>%s</%s>", tag, strings.Join(inner, "<br>"), tag)
		}
		b.WriteString("</tr>\n")
	}
	b.WriteString("</table>\n")
}

// inlineHTML renders one rich-text line, keeping the marks Markdown carries.
func inlineHTML(t doc.RichText) string {
	var b strings.Builder
	link := ""
	for _, sp := range t.Spans() {
		src := t.Text[sp.Start:sp.End]
		var text string
		switch {
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
		if sp.Marks.Link != nil && *sp.Marks.Link != link {
			link = *sp.Marks.Link
		}
		if link != "" && !sp.Marks.Code && sp.Marks.Inline == nil {
			text = "<a href=\"" + html.EscapeString(link) + "\">" + text + "</a>"
		}
		b.WriteString(text)
	}
	// A paragraph's own newlines are hard breaks.
	return strings.ReplaceAll(b.String(), "\n", "<br>")
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
