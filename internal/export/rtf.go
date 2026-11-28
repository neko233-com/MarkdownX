package export

import (
	"fmt"
	"strings"
	"unicode/utf16"
	"unicode/utf8"

	"github.com/neko233/markdownx/internal/doc"
)

// RichTextFormat renders the text and basic formatting of a clipboard fragment.
func RichTextFormat(d *doc.Document) string {
	var b strings.Builder
	b.WriteString(`{\rtf1\ansi\ansicpg1252\uc1\deff0{\fonttbl{\f0\fswiss Arial;}{\f1\fmodern Courier New;}}`)
	started := false
	paragraph := func(text doc.RichText, style, marker string) {
		if started {
			b.WriteString("\\par\n")
		}
		started = true
		b.WriteString("{\\pard\\f0 ")
		b.WriteString(style)
		writeRTFString(&b, marker)
		for _, span := range text.Spans() {
			m := span.Marks
			b.WriteByte('{')
			if m.Bold {
				b.WriteString(`\b `)
			}
			if m.Italic {
				b.WriteString(`\i `)
			}
			if m.Underline {
				b.WriteString(`\ul `)
			}
			if m.Strike {
				b.WriteString(`\strike `)
			}
			if m.Code {
				b.WriteString(`\f1 `)
			}
			value := text.Text[span.Start:span.End]
			if m.Inline != nil {
				part := text.Slice(span.Start, span.End)
				value = part.PlainText()
			}
			writeRTFString(&b, value)
			b.WriteByte('}')
		}
		b.WriteByte('}')
	}
	if !d.Title.IsEmpty() {
		paragraph(d.Title, `\b\fs36 `, "")
	}
	d.ForEachBlock(func(blk *doc.Block) {
		style, marker := "", ""
		switch k := blk.Kind.(type) {
		case doc.HeadingKind:
			style = fmt.Sprintf(`\b\fs%d `, 36-2*min(6, int(k.Level)))
		case doc.BulletKind, doc.OrderedKind:
			marker = "• "
		case doc.TodoKind:
			marker = "☐ "
			if k.Done {
				marker = "☑ "
			}
		case doc.CodeKind:
			style = `\f1 `
		}
		if blk.Kind.HasText() {
			paragraph(blk.Text, style, marker)
		}
	})
	b.WriteByte('}')
	return b.String()
}

func writeRTFString(b *strings.Builder, text string) {
	for _, r := range text {
		switch {
		case r == '\\' || r == '{' || r == '}':
			b.WriteByte('\\')
			b.WriteRune(r)
		case r == '\n':
			b.WriteString(`\line `)
		case r == '\t':
			b.WriteString(`\tab `)
		case r >= ' ' && r < utf8.RuneSelf:
			b.WriteRune(r)
		case r >= 0x10000:
			hi, lo := utf16.EncodeRune(r)
			fmt.Fprintf(b, `\u%d?\u%d?`, int16(hi), int16(lo))
		case r >= utf8.RuneSelf:
			fmt.Fprintf(b, `\u%d?`, int16(r))
		}
	}
}
