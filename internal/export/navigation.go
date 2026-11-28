package export

import (
	"fmt"
	"html"
	"strconv"
	"strings"

	"github.com/neko233/markdownx/internal/doc"
)

type documentHTML struct {
	*strings.Builder
	diagrams bool
	headings []doc.Heading
	anchors  map[doc.BlockID]string
	used     map[string]bool
	refs     doc.References
	notes    map[doc.BlockID]*htmlFootnote
}

type htmlFootnote struct {
	id        string
	backlinks []string
}

func (w *documentHTML) heading(id doc.BlockID, level uint8, t doc.RichText) {
	attr := ""
	if anchor := w.anchors[id]; anchor != "" {
		attr = ` id="` + html.EscapeString(anchor) + `"`
	}
	fmt.Fprintf(w, "<h%d%s>%s</h%d>\n", level, attr, w.inline(t, true), level)
}

func (w *documentHTML) contents() {
	if len(w.headings) == 0 {
		return
	}
	w.WriteString("<nav class=\"toc\" aria-label=\"目录\">\n")
	at := 0
	w.contentsList(&at, 0)
	w.WriteString("</nav>\n")
}

func (w *documentHTML) contentsList(at *int, parent uint8) {
	w.WriteString("<ul>\n")
	for *at < len(w.headings) && w.headings[*at].Level > parent {
		h := w.headings[*at]
		*at = *at + 1
		text := h.Text
		if text == "" {
			text = "空标题"
		}
		fmt.Fprintf(w, "<li>%s", htmlLink(html.EscapeString(text), "#"+h.Slug, ""))
		if *at < len(w.headings) && w.headings[*at].Level > h.Level {
			w.contentsList(at, h.Level)
		}
		w.WriteString("</li>\n")
	}
	w.WriteString("</ul>\n")
}

func htmlLink(text, destination, title string) string {
	if strings.HasPrefix(strings.ToLower(destination), "www.") {
		destination = "https://" + destination
	}
	if !doc.LinkOK(destination) && !doc.RelativeLinkOK(destination) {
		return text
	}
	attr := ""
	if title != "" {
		attr = ` title="` + html.EscapeString(title) + `"`
	}
	return `<a href="` + html.EscapeString(destination) + `"` + attr + `>` + text + `</a>`
}

func (w *documentHTML) reserveID(base string) string {
	id := base
	for n := 1; w.used[id]; n++ {
		id = base + "-" + strconv.Itoa(n)
	}
	w.used[id] = true
	return id
}

func (w *documentHTML) reference(ref doc.ReferenceInline, links bool) string {
	if !links {
		return html.EscapeString(ref.PlainText())
	}
	if !ref.Footnote {
		if target, ok := w.refs.Link(ref.Label); ok {
			if ref.Image {
				if !doc.LinkOK(target.Destination) && !doc.RelativePathOK(target.Destination) && !strings.HasPrefix(target.Destination, "data:image/") {
					return html.EscapeString(ref.PlainText())
				}
				title := ""
				if target.Title != "" {
					title = ` title="` + html.EscapeString(target.Title) + `"`
				}
				return `<img src="` + html.EscapeString(target.Destination) + `" alt="` + html.EscapeString(ref.PlainText()) + `"` + title + `>`
			}
			return htmlLink(w.inline(doc.ParseInlineSource(ref.Caption), false), target.Destination, target.Title)
		}
		return html.EscapeString(ref.Markup)
	}
	note, ok := w.refs.Footnote(ref.Label)
	if !ok || note.Number == 0 {
		return html.EscapeString(ref.Markup)
	}
	number := strconv.Itoa(note.Number)
	anchors := w.notes[note.Block]
	if anchors == nil {
		anchors = &htmlFootnote{id: w.reserveID("fn-" + number)}
		w.notes[note.Block] = anchors
	}
	id := w.reserveID("fnref-" + number + "-" + strconv.Itoa(len(anchors.backlinks)+1))
	anchors.backlinks = append(anchors.backlinks, id)
	return `<sup><a id="` + id + `" href="#` + anchors.id + `" role="doc-noteref">` + number + `</a></sup>`
}

func (w *documentHTML) footnotes() {
	notes := w.refs.Footnotes()
	if len(notes) == 0 {
		return
	}
	out := w.Builder
	bodies := make([]string, len(notes))
	for i, note := range notes {
		var body strings.Builder
		w.Builder = &body
		for _, b := range note.Body {
			w.block(b)
		}
		bodies[i] = body.String()
	}
	w.Builder = out
	w.WriteString("<section class=\"footnotes\" role=\"doc-endnotes\" aria-label=\"脚注\"><ol>\n")
	for i, note := range notes {
		anchors := w.notes[note.Block]
		if anchors == nil {
			continue
		}
		fmt.Fprintf(w, "<li id=\"%s\">\n%s<div class=\"footnote-backlinks\">", anchors.id, bodies[i])
		for _, id := range anchors.backlinks {
			fmt.Fprintf(w, " <a href=\"#%s\" role=\"doc-backlink\" aria-label=\"返回脚注引用\">↩</a>", id)
		}
		w.WriteString("</div></li>\n")
	}
	w.WriteString("</ol></section>\n")
}
