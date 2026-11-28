package book

import (
	"strconv"
	"strings"

	"github.com/neko233/markdownx/internal/doc"
)

// node is one element or text run of a DocxXML chapter.
type node struct {
	tag   string // "" for text
	attrs map[string]string
	kids  []*node
	text  string
}

func (n *node) attr(key string) string {
	if n.attrs == nil {
		return ""
	}
	return n.attrs[key]
}

func (n *node) attrAny(keys ...string) string {
	for _, k := range keys {
		if v := n.attr(k); v != "" {
			return v
		}
	}
	return ""
}

// textOf is the concatenated text of the subtree.
func (n *node) textOf() string {
	var b strings.Builder
	var walk func(*node)
	walk = func(x *node) {
		if x.tag == "" {
			b.WriteString(x.text)
			return
		}
		if x.tag == "br" {
			b.WriteByte('\n')
		}
		for _, k := range x.kids {
			walk(k)
		}
	}
	walk(n)
	return b.String()
}

// ParseDocxXML parses chapter content into document blocks.
func ParseDocxXML(src string) []*doc.Block {
	nodes := tokenize(src)
	var out []*doc.Block
	// Consecutive inline nodes belong to one paragraph.
	var pending []*node
	flush := func() {
		if len(pending) == 0 {
			return
		}
		t := inlineOf(pending, doc.Marks{})
		pending = nil
		if strings.TrimSpace(t.Text) != "" {
			out = append(out, doc.NewBlock(doc.TextKind{}, t))
		}
	}
	for _, n := range nodes {
		if n.tag == "" {
			pending = append(pending, n)
			continue
		}
		if !isBlockTag(n.tag) {
			pending = append(pending, n)
			continue
		}
		flush()
		out = append(out, blocksOf(n)...)
	}
	flush()
	return out
}

func isBlockTag(tag string) bool {
	switch tag {
	case "h1", "h2", "h3", "h4", "h5", "h6", "p", "div", "ul", "ol", "li",
		"blockquote", "callout", "checkbox", "todo", "pre", "table", "grid",
		"column", "interactive", "image", "hr", "figure", "sub-page-list",
		"countdown", "section", "article", "markdown-toc", "markdown-definition":
		return true
	}
	return false
}

// blocksOf turns one element into blocks.
func blocksOf(n *node) []*doc.Block {
	switch n.tag {
	case "h1", "h2", "h3", "h4", "h5", "h6":
		level, _ := strconv.Atoi(n.tag[1:])
		t := inlineOf(n.kids, doc.Marks{})
		return []*doc.Block{doc.NewBlock(doc.HeadingKind{Level: uint8(level)}, t)}
	case "p":
		t := inlineOf(n.kids, doc.Marks{})
		if t.Text == "" && len(t.Runs) == 0 {
			return nil
		}
		return []*doc.Block{doc.NewBlock(doc.TextKind{}, t)}
	case "div", "section", "article", "figure":
		return childrenBlocks(n)
	case "ul":
		return listBlocks(n, false)
	case "ol":
		return listBlocks(n, true)
	case "li":
		return []*doc.Block{doc.NewBlock(doc.BulletKind{}, inlineOf(n.kids, doc.Marks{}))}
	case "checkbox", "todo":
		done := n.attr("done") == "true" || n.attr("checked") == "true"
		return []*doc.Block{doc.NewBlock(doc.TodoKind{Done: done}, doc.PlainRich(n.textOf()))}
	case "blockquote":
		kids := childrenBlocks(n)
		if len(kids) == 0 {
			return nil
		}
		return []*doc.Block{doc.NewBlock(doc.QuoteKind{}, doc.RichText{}).WithChildren(kids...)}
	case "callout":
		kids := childrenBlocks(n)
		k := doc.CalloutKind{
			Emoji:  n.attr("emoji"),
			BG:     strOrNil(n.attrAny("background-color", "bg-color")),
			Border: strOrNil(n.attrAny("border-color", "border")),
		}
		if c := n.attr("text-color"); c != "" {
			k.TextColor = &c
		}
		return []*doc.Block{doc.NewBlock(k, doc.RichText{}).WithChildren(kids...)}
	case "pre":
		code := ""
		var caption *string
		if c := n.attr("caption"); c != "" {
			caption = &c
		}
		for _, k := range n.kids {
			if k.tag == "code" {
				code = k.textOf()
				break
			}
		}
		if code == "" {
			code = n.textOf()
		}
		code = strings.TrimPrefix(code, "\n")
		code = strings.TrimSuffix(code, "\n")
		lang := n.attrAny("lang", "language")
		return []*doc.Block{doc.NewBlock(doc.CodeKind{Lang: lang, Caption: caption}, doc.PlainRich(code))}
	case "table":
		return []*doc.Block{tableBlock(n)}
	case "grid":
		var cols []*doc.Block
		for _, k := range n.kids {
			if k.tag != "column" {
				continue
			}
			ratio := float32(1)
			if v := k.attrAny("width-ratio", "ratio"); v != "" {
				if f, err := strconv.ParseFloat(v, 32); err == nil && f > 0 {
					ratio = float32(f)
				}
			}
			cols = append(cols, doc.NewBlock(doc.ColumnKind{Ratio: ratio}, doc.RichText{}).WithChildren(childrenBlocks(k)...))
		}
		if len(cols) == 0 {
			return nil
		}
		return []*doc.Block{doc.NewBlock(doc.GridKind{}, doc.RichText{}).WithChildren(cols...)}
	case "column":
		return childrenBlocks(n)
	case "interactive":
		source := ""
		for _, k := range n.kids {
			source += k.text
		}
		if source == "" {
			source = n.attr("source")
		}
		height := uint32(360)
		if v := n.attr("height"); v != "" {
			if f, err := strconv.Atoi(v); err == nil && f > 0 {
				height = uint32(f)
			}
		}
		kind := doc.ParseInteractiveType(n.attr("type"))
		return []*doc.Block{doc.NewBlock(doc.InteractiveKindBlock{Interactive: doc.Interactive{
			Kind: kind, Title: n.attr("title"), Height: height, Source: source,
		}}, doc.RichText{})}
	case "image":
		src := n.attrAny("src", "path", "href")
		if src == "" {
			return nil
		}
		kind := n.attr("src-kind")
		if kind == "" {
			if n.attr("path") != "" {
				kind = "path"
			} else {
				kind = "src"
			}
		}
		img := doc.ImageKind{SrcKind: kind, Src: src}
		if v := n.attr("width"); v != "" {
			if f, err := strconv.ParseFloat(v, 32); err == nil {
				p := float32(f)
				img.Width = &p
			}
		}
		if v := n.attr("height"); v != "" {
			if f, err := strconv.ParseFloat(v, 32); err == nil {
				p := float32(f)
				img.Height = &p
			}
		}
		if c := n.attrAny("caption", "alt"); c != "" {
			img.Caption = &c
		}
		b := doc.NewBlock(img, doc.RichText{})
		b.Align = doc.AlignCenter
		return []*doc.Block{b}
	case "hr":
		return []*doc.Block{doc.NewBlock(doc.DividerKind{}, doc.RichText{})}
	case "markdown-toc":
		return []*doc.Block{doc.NewBlock(doc.TOCKind{Marker: n.attr("source")}, doc.RichText{})}
	case "markdown-definition":
		return []*doc.Block{doc.NewBlock(doc.DefinitionKind{}, doc.PlainRich(n.textOf()))}
	case "sub-page-list":
		return []*doc.Block{doc.NewBlock(doc.SubPageListKind{}, doc.RichText{})}
	case "countdown":
		return []*doc.Block{doc.NewBlock(doc.CountdownKind{Countdown: doc.DefaultCountdown()}, doc.RichText{})}
	}
	return childrenBlocks(n)
}

func childrenBlocks(n *node) []*doc.Block {
	var out []*doc.Block
	var pending []*node
	flush := func() {
		if len(pending) == 0 {
			return
		}
		t := inlineOf(pending, doc.Marks{})
		pending = nil
		if strings.TrimSpace(t.Text) != "" {
			out = append(out, doc.NewBlock(doc.TextKind{}, t))
		}
	}
	for _, k := range n.kids {
		if k.tag == "" || !isBlockTag(k.tag) {
			pending = append(pending, k)
			continue
		}
		flush()
		out = append(out, blocksOf(k)...)
	}
	flush()
	return out
}

func listBlocks(n *node, ordered bool) []*doc.Block {
	var out []*doc.Block
	for _, li := range n.kids {
		if li.tag != "li" {
			continue
		}
		// A checkbox inside an item makes it a task.
		if done, ok := itemTodo(li); ok {
			out = append(out, doc.NewBlock(doc.TodoKind{Done: done}, doc.PlainRich(itemText(li))).WithChildren(itemExtra(li)...))
			continue
		}
		b := doc.NewBlock(doc.BulletKind{}, doc.PlainRich(itemText(li)))
		if ordered {
			b = doc.NewBlock(doc.OrderedKind{}, doc.PlainRich(itemText(li)))
		}
		b.Children = itemExtra(li)
		out = append(out, b)
	}
	return out
}

func itemTodo(li *node) (bool, bool) {
	for _, k := range li.kids {
		if k.tag == "checkbox" || k.tag == "todo" {
			return k.attr("done") == "true" || k.attr("checked") == "true", true
		}
	}
	return false, false
}

func itemText(li *node) string {
	for _, k := range li.kids {
		if k.tag == "" || k.tag == "checkbox" || k.tag == "todo" {
			continue
		}
		if !isBlockTag(k.tag) {
			return inlineOf(li.kids, doc.Marks{}).Text
		}
	}
	return li.textOf()
}

func itemExtra(li *node) []*doc.Block {
	var out []*doc.Block
	for _, k := range li.kids {
		if k.tag == "ul" || k.tag == "ol" || k.tag == "pre" || k.tag == "blockquote" || k.tag == "table" {
			out = append(out, blocksOf(k)...)
		}
	}
	return out
}

func tableBlock(n *node) *doc.Block {
	var rows [][]doc.TableCell
	headerRows := 0
	var walkRows func(*node, bool)
	walkRows = func(parent *node, header bool) {
		for _, k := range parent.kids {
			switch k.tag {
			case "thead":
				walkRows(k, true)
			case "tbody", "tfoot":
				walkRows(k, false)
			case "tr":
				var row []doc.TableCell
				for _, cell := range k.kids {
					if cell.tag != "th" && cell.tag != "td" {
						continue
					}
					blocks := childrenBlocks(cell)
					if len(blocks) == 0 {
						blocks = []*doc.Block{doc.TextBlock("")}
					}
					row = append(row, doc.TableCell{Blocks: blocks, ColSpan: 1, RowSpan: 1})
				}
				if len(row) > 0 {
					rows = append(rows, row)
					if header {
						headerRows = len(rows)
					}
				}
			}
		}
	}
	walkRows(n, false)
	t := &doc.Table{HeaderRows: headerRows, Rows: rows}
	if len(rows) > 0 {
		widths := make([]float32, len(rows[0]))
		for i := range widths {
			widths[i] = 200
		}
		t.ColWidths = widths
	}
	t.Repair()
	return doc.NewBlock(doc.TableKind{Table: t}, doc.RichText{})
}

// inlineOf renders a list of nodes as one rich-text line.
func inlineOf(nodes []*node, marks doc.Marks) doc.RichText {
	var out doc.RichText
	for _, n := range nodes {
		if n.tag == "" {
			if n.text != "" {
				out.Push(n.text, marks)
			}
			continue
		}
		switch n.tag {
		case "br":
			out.Push("\n", marks)
		case "b", "strong":
			m := marks
			m.Bold = true
			out.Append(inlineOf(n.kids, m))
		case "i", "em":
			m := marks
			m.Italic = true
			out.Append(inlineOf(n.kids, m))
		case "u":
			m := marks
			m.Underline = true
			out.Append(inlineOf(n.kids, m))
		case "s", "del", "strike":
			m := marks
			m.Strike = true
			out.Append(inlineOf(n.kids, m))
		case "code":
			out.Push(n.textOf(), withCode(marks))
		case "latex":
			src := n.textOf()
			in := doc.EquationInlineObj(src)
			out.Push(doc.Obj, doc.Marks{Inline: &in})
		case "a":
			m := marks
			if href := n.attr("href"); href != "" {
				m.Link = &href
			}
			out.Append(inlineOf(n.kids, m))
		case "markdown-reference":
			source := n.attr("source")
			if in, ok := doc.ReferenceObject(source); ok {
				m := marks
				m.Inline = &in
				out.Push(doc.Obj, m)
			} else {
				out.Push(source, marks)
			}
		case "image":
			src := n.attrAny("src", "path", "href")
			if src == "" {
				continue
			}
			img := doc.InlineImage{SrcKind: n.attrAny("src-kind", "kind"), Src: src, Alt: n.attrAny("alt", "caption")}
			if img.SrcKind == "" {
				img.SrcKind = "src"
			}
			in := doc.ImageInlineObj(img)
			out.Push(doc.Obj, doc.Marks{Inline: &in})
		case "button":
			action := doc.OpenLink
			switch n.attr("action") {
			case "DuplicatePage":
				action = doc.DuplicatePage
			case "FollowPage":
				action = doc.FollowPage
			}
			btn := doc.InlineButton{Text: n.textOf(), Action: action, Src: n.attr("src"), Color: n.attrAny("background-color", "color")}
			if btn.Text == "" {
				btn.Text = action.Label()
			}
			if btn.Color == "" {
				btn.Color = doc.ButtonDefaultColor
			}
			in := doc.Inline{Kind: doc.ButtonInline{Button: btn}, XML: buttonXML(btn)}
			out.Push(doc.Obj, doc.Marks{Inline: &in})
		case "time":
			r := doc.InlineReminder{}
			if v := n.attr("expire"); v != "" {
				r.Expire, _ = strconv.ParseInt(v, 10, 64)
			}
			if v := n.attr("notify"); v != "" {
				r.Notify, _ = strconv.ParseInt(v, 10, 64)
			}
			r.ShouldNotify = n.attr("should-notify") == "true"
			r.WholeDay = n.attr("is-whole-day") == "true"
			in := doc.Inline{Kind: doc.ReminderInline{Reminder: r}, XML: reminderXML(r)}
			out.Push(doc.Obj, doc.Marks{Inline: &in})
		case "cite":
			l := doc.InlineDocLink{ID: n.attr("doc-id"), Title: n.textOf()}
			in := doc.Inline{Kind: doc.DocLinkInline{Link: l}, XML: docLinkXML(l)}
			out.Push(doc.Obj, doc.Marks{Inline: &in})
		case "source":
			f := doc.FileBlock{SrcKind: "path", Src: n.attrAny("path", "src"), Name: n.attr("name")}
			if f.Src == "" {
				continue
			}
			if f.Name == "" {
				f.Name = f.Src
			}
			in := doc.Inline{Kind: doc.FileInline{File: f}, XML: fileXML(f)}
			out.Push(doc.Obj, doc.Marks{Inline: &in})
		case "checkbox", "todo":
			// Inline checkboxes read as their text.
			out.Push(n.textOf(), marks)
		default:
			out.Append(inlineOf(n.kids, marks))
		}
	}
	return out
}

func withCode(m doc.Marks) doc.Marks {
	m.Code = true
	m.Link = nil
	return m
}

func strOrNil(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}

// ---- serialization

// ToDocxXML writes blocks as DocxXML chapter content.
func ToDocxXML(blocks []*doc.Block) string {
	var b strings.Builder
	writeBlocks(&b, blocks)
	return strings.TrimSpace(b.String())
}

func writeBlocks(b *strings.Builder, blocks []*doc.Block) {
	for i := 0; i < len(blocks); i++ {
		blk := blocks[i]
		// Group consecutive list items into one list element.
		switch blk.Kind.(type) {
		case doc.BulletKind, doc.OrderedKind:
			ordered := false
			if _, ok := blk.Kind.(doc.OrderedKind); ok {
				ordered = true
			}
			tag := "ul"
			if ordered {
				tag = "ol"
			}
			b.WriteString("<" + tag + ">")
			for i < len(blocks) {
				cur := blocks[i]
				curOrdered := false
				if _, ok := cur.Kind.(doc.OrderedKind); ok {
					curOrdered = true
				}
				if curOrdered != ordered || (curOrdered && ordered && isListBreak(cur)) {
					break
				}
				if _, ok := cur.Kind.(doc.BulletKind); !ok && !curOrdered {
					break
				}
				b.WriteString("<li>" + inlineXML(cur.Text) + "</li>")
				i++
			}
			b.WriteString("</" + tag + ">")
			i--
			continue
		}
		writeBlock(b, blk)
	}
}

func isListBreak(*doc.Block) bool { return false }

func writeBlock(b *strings.Builder, blk *doc.Block) {
	switch k := blk.Kind.(type) {
	case doc.TextKind:
		if strings.TrimSpace(blk.Text.Text) == "" {
			return
		}
		b.WriteString("<p>" + inlineXML(blk.Text) + "</p>")
	case doc.HeadingKind:
		lvl := int(k.Level)
		if lvl < 1 {
			lvl = 1
		}
		if lvl > 6 {
			lvl = 6
		}
		tag := "h" + strconv.Itoa(lvl)
		b.WriteString("<" + tag + ">" + inlineXML(blk.Text) + "</" + tag + ">")
	case doc.TodoKind:
		done := "false"
		if k.Done {
			done = "true"
		}
		b.WriteString(`<checkbox done="` + done + `">` + escapeXML(blk.Text.Text) + `</checkbox>`)
	case doc.CodeKind:
		attrs := ""
		if k.Lang != "" {
			attrs += ` lang="` + escapeAttr(k.Lang) + `"`
		}
		if k.Caption != nil {
			attrs += ` caption="` + escapeAttr(*k.Caption) + `"`
		}
		b.WriteString("<pre" + attrs + "><code>" + escapeXML(blk.Text.Text) + "</code></pre>")
	case doc.QuoteKind:
		b.WriteString("<blockquote>")
		writeBlocks(b, blk.Children)
		b.WriteString("</blockquote>")
	case doc.CalloutKind:
		attrs := ""
		if k.Emoji != "" {
			attrs += ` emoji="` + escapeAttr(k.Emoji) + `"`
		}
		if k.BG != nil {
			attrs += ` background-color="` + escapeAttr(*k.BG) + `"`
		}
		if k.Border != nil {
			attrs += ` border-color="` + escapeAttr(*k.Border) + `"`
		}
		if k.TextColor != nil {
			attrs += ` text-color="` + escapeAttr(*k.TextColor) + `"`
		}
		b.WriteString("<callout" + attrs + ">")
		writeBlocks(b, blk.Children)
		b.WriteString("</callout>")
	case doc.DividerKind:
		b.WriteString("<hr/>")
	case doc.TOCKind:
		b.WriteString(`<markdown-toc source="` + escapeAttr(k.Marker) + `"/>`)
	case doc.DefinitionKind:
		b.WriteString("<markdown-definition>" + escapeXML(blk.Text.Text) + "</markdown-definition>")
	case doc.ImageKind:
		attrs := ` src="` + escapeAttr(k.Src) + `" src-kind="` + escapeAttr(k.SrcKind) + `"`
		if k.Caption != nil {
			attrs += ` caption="` + escapeAttr(*k.Caption) + `"`
		}
		if k.Width != nil {
			attrs += ` width="` + ftoa(*k.Width) + `"`
		}
		if k.Height != nil {
			attrs += ` height="` + ftoa(*k.Height) + `"`
		}
		b.WriteString("<image" + attrs + "/>")
	case doc.TableKind:
		writeTable(b, k.Table)
	case doc.GridKind:
		b.WriteString("<grid>")
		for _, col := range blk.Children {
			ratio := float32(1)
			if ck, ok := col.Kind.(doc.ColumnKind); ok {
				ratio = ck.Ratio
			}
			b.WriteString(`<column width-ratio="` + ftoa(ratio) + `">`)
			writeBlocks(b, col.Children)
			b.WriteString("</column>")
		}
		b.WriteString("</grid>")
	case doc.InteractiveKindBlock:
		height := k.Interactive.VisibleHeight()
		b.WriteString(`<interactive type="` + k.Interactive.Kind.String() + `" title="` +
			escapeAttr(k.Interactive.Title) + `" height="` + strconv.FormatUint(uint64(height), 10) + `">`)
		b.WriteString("<![CDATA[" + strings.ReplaceAll(k.Interactive.Source, "]]>", "]]]]><![CDATA[>") + "]]>")
		b.WriteString("</interactive>")
	case doc.FileKind:
		b.WriteString(`<source path="` + escapeAttr(k.File.Src) + `" name="` + escapeAttr(k.File.Name) + `"/>`)
	case doc.SubPageListKind:
		b.WriteString("<sub-page-list/>")
	case doc.CountdownKind:
		b.WriteString("<countdown/>")
	case doc.RawKind:
		b.WriteString(k.XML)
	}
}

func writeTable(b *strings.Builder, t *doc.Table) {
	if t == nil || len(t.Rows) == 0 {
		return
	}
	b.WriteString("<table>")
	header := t.HeaderRows
	if header <= 0 {
		header = 1
	}
	writeRows := func(rows [][]doc.TableCell, cellTag string) {
		for _, row := range rows {
			b.WriteString("<tr>")
			for _, cell := range row {
				if cell.IsCovered() {
					continue
				}
				b.WriteString("<" + cellTag + ">")
				blocks := cell.Blocks
				if len(blocks) == 0 {
					blocks = []*doc.Block{doc.TextBlock("")}
				}
				writeBlocks(b, blocks)
				b.WriteString("</" + cellTag + ">")
			}
			b.WriteString("</tr>")
		}
	}
	b.WriteString("<thead>")
	writeRows(t.Rows[:min(header, len(t.Rows))], "th")
	b.WriteString("</thead><tbody>")
	writeRows(t.Rows[min(header, len(t.Rows)):], "td")
	b.WriteString("</tbody></table>")
}

func inlineXML(t doc.RichText) string {
	var b strings.Builder
	for _, sp := range t.Spans() {
		src := t.Text[sp.Start:sp.End]
		switch {
		case sp.Marks.Inline != nil && isEquation(sp.Marks.Inline):
			s, _ := sp.Marks.Inline.Equation()
			b.WriteString("<latex>" + escapeXML(s) + "</latex>")
			continue
		case sp.Marks.Inline != nil && isImage(sp.Marks.Inline):
			img, _ := sp.Marks.Inline.Image()
			b.WriteString(`<image src="` + escapeAttr(img.Src) + `" alt="` + escapeAttr(img.Alt) + `"/>`)
			continue
		case sp.Marks.Inline != nil:
			b.WriteString(sp.Marks.Inline.XML)
			continue
		}
		x := escapeXML(src)
		if sp.Marks.Code {
			x = "<code>" + x + "</code>"
		}
		if sp.Marks.Underline {
			x = "<u>" + x + "</u>"
		}
		if sp.Marks.Strike {
			x = "<s>" + x + "</s>"
		}
		if sp.Marks.Italic {
			x = "<i>" + x + "</i>"
		}
		if sp.Marks.Bold {
			x = "<b>" + x + "</b>"
		}
		if sp.Marks.Link != nil {
			x = `<a href="` + escapeAttr(*sp.Marks.Link) + `">` + x + `</a>`
		}
		b.WriteString(x)
	}
	return b.String()
}

func isEquation(in *doc.Inline) bool {
	_, ok := in.Equation()
	return ok
}

func isImage(in *doc.Inline) bool {
	_, ok := in.Image()
	return ok
}

func escapeXML(s string) string {
	r := strings.NewReplacer("&", "&amp;", "<", "&lt;", ">", "&gt;")
	return r.Replace(s)
}

func escapeAttr(s string) string {
	r := strings.NewReplacer("&", "&amp;", "<", "&lt;", ">", "&gt;", `"`, "&quot;", "\n", "&#10;")
	return r.Replace(s)
}

func ftoa(f float32) string {
	return strconv.FormatFloat(float64(f), 'f', -1, 32)
}

func buttonXML(b doc.InlineButton) string {
	return `<button action="` + b.Action.Name() + `" src="` + escapeAttr(b.Src) +
		`" background-color="` + escapeAttr(b.Color) + `">` + escapeXML(b.Text) + `</button>`
}

func reminderXML(r doc.InlineReminder) string {
	s := `<time expire="` + strconv.FormatInt(r.Expire, 10) + `" notify="` + strconv.FormatInt(r.Notify, 10) + `"`
	if r.ShouldNotify {
		s += ` should-notify="true"`
	}
	if r.WholeDay {
		s += ` is-whole-day="true"`
	}
	return s + `</time>`
}

func docLinkXML(l doc.InlineDocLink) string {
	return `<cite type="doc" doc-id="` + escapeAttr(l.ID) + `">` + escapeXML(l.Title) + `</cite>`
}

func fileXML(f doc.FileBlock) string {
	return `<source path="` + escapeAttr(f.Src) + `" name="` + escapeAttr(f.Name) + `"/>`
}

// ---- tokenizer

// tokenize splits DocxXML content into a node tree. It is deliberately
// forgiving: unclosed tags close at the end, and stray '<' stays text.
func tokenize(s string) []*node {
	root := &node{tag: "#root"}
	stack := []*node{root}
	top := func() *node { return stack[len(stack)-1] }
	add := func(n *node) { top().kids = append(top().kids, n) }

	appendText := func(t string, raw bool) {
		if t == "" {
			return
		}
		if !raw {
			// Entities are decoded once, as they are read; CDATA is code.
			t = unescapeEntities(t)
		}
		if len(top().kids) > 0 {
			if last := top().kids[len(top().kids)-1]; last.tag == "" && last.attrs == nil {
				last.text += t
				return
			}
		}
		n := &node{text: t}
		if raw {
			n.attrs = map[string]string{"raw": "1"}
		}
		add(n)
	}

	i := 0
	for i < len(s) {
		c := s[i]
		if c != '<' {
			j := strings.IndexByte(s[i:], '<')
			if j < 0 {
				appendText(s[i:], false)
				break
			}
			appendText(s[i:i+j], false)
			i += j
			continue
		}
		if strings.HasPrefix(s[i:], "<!--") {
			if end := strings.Index(s[i:], "-->"); end >= 0 {
				i += end + 3
				continue
			}
			break
		}
		if strings.HasPrefix(s[i:], "<![CDATA[") {
			body := s[i+9:]
			end := strings.Index(body, "]]>")
			if end < 0 {
				appendText(body, true)
				break
			}
			appendText(body[:end], true)
			i += 9 + end + 3
			continue
		}
		gt := strings.IndexByte(s[i:], '>')
		if gt < 0 {
			appendText(s[i:], false)
			break
		}
		raw := s[i+1 : i+gt]
		i += gt + 1
		if strings.HasPrefix(raw, "/") {
			name := strings.ToLower(strings.TrimSpace(raw[1:]))
			for k := len(stack) - 1; k > 0; k-- {
				if stack[k].tag == name {
					stack = stack[:k]
					break
				}
			}
			continue
		}
		selfClosing := strings.HasSuffix(strings.TrimSpace(raw), "/")
		raw = strings.TrimSuffix(strings.TrimSpace(raw), "/")
		name, attrs, _ := splitTag(raw)
		if name == "" {
			continue
		}
		n := &node{tag: name, attrs: attrs}
		add(n)
		if !selfClosing && !isVoidTag(name) {
			stack = append(stack, n)
		}
	}
	return root.kids
}

func isVoidTag(tag string) bool {
	switch tag {
	case "br", "hr", "image", "img", "source", "sub-page-list", "countdown", "input":
		return true
	}
	return false
}

func splitTag(raw string) (string, map[string]string, bool) {
	i := 0
	for i < len(raw) && !isSpace(raw[i]) {
		i++
	}
	name := strings.ToLower(raw[:i])
	attrs := map[string]string{}
	for i < len(raw) {
		for i < len(raw) && isSpace(raw[i]) {
			i++
		}
		if i >= len(raw) {
			break
		}
		start := i
		for i < len(raw) && raw[i] != '=' && !isSpace(raw[i]) {
			i++
		}
		key := strings.ToLower(raw[start:i])
		for i < len(raw) && isSpace(raw[i]) {
			i++
		}
		if i < len(raw) && raw[i] == '=' {
			i++
			for i < len(raw) && isSpace(raw[i]) {
				i++
			}
			var val string
			if i < len(raw) && (raw[i] == '"' || raw[i] == '\'') {
				q := raw[i]
				i++
				vs := i
				for i < len(raw) && raw[i] != q {
					i++
				}
				val = raw[vs:i]
				if i < len(raw) {
					i++
				}
			} else {
				vs := i
				for i < len(raw) && !isSpace(raw[i]) {
					i++
				}
				val = raw[vs:i]
			}
			if key != "" {
				attrs[key] = unescapeEntities(val)
			}
			continue
		}
		if key != "" {
			attrs[key] = ""
		}
	}
	return name, attrs, true
}

func isSpace(c byte) bool { return c == ' ' || c == '\t' || c == '\n' || c == '\r' }

func unescapeEntities(s string) string {
	r := strings.NewReplacer(
		"&lt;", "<", "&gt;", ">", "&quot;", `"`, "&#39;", "'", "&apos;", "'",
		"&#10;", "\n", "&#9;", "\t",
	)
	return r.Replace(strings.ReplaceAll(s, "&amp;", "&"))
}
