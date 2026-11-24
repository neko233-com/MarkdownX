package convert

import (
	"archive/zip"
	"bytes"
	"encoding/xml"
	"fmt"
	"io"
	"strconv"
	"strings"
)

// Docx turns a .docx (WordprocessingML) archive into Markdown source.
//
// The document part is streamed token by token, so a large file is never built
// into a DOM. Only the small relationship and numbering parts are decoded up
// front; both are optional.
func Docx(data []byte) (string, error) {
	zr, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		return "", fmt.Errorf("docx: %w", err)
	}
	d := &docReader{
		files:     make(map[string]*zip.File, len(zr.File)),
		rels:      make(map[string]string),
		numbering: make(map[numKey]bool),
	}
	for _, f := range zr.File {
		d.files[f.Name] = f
	}
	if err := d.loadRels("word/_rels/document.xml.rels"); err != nil {
		return "", err
	}
	d.loadNumbering("word/numbering.xml")
	doc := d.files["word/document.xml"]
	if doc == nil {
		return "", fmt.Errorf("docx: word/document.xml not found")
	}
	rc, err := doc.Open()
	if err != nil {
		return "", fmt.Errorf("docx: %w", err)
	}
	defer rc.Close()
	dec := xml.NewDecoder(rc)
	dec.Strict = false
	if err := d.parse(dec); err != nil {
		return "", err
	}
	out := d.render()
	if out != "" {
		out += "\n"
	}
	return out, nil
}

// render joins blocks: a blank line between them, except between consecutive
// list items, which form one list.
func (d *docReader) render() string {
	var out strings.Builder
	for i, b := range d.blocks {
		if i > 0 {
			if b.list && d.blocks[i-1].list {
				out.WriteByte('\n')
			} else {
				out.WriteString("\n\n")
			}
		}
		out.WriteString(b.text)
	}
	return out.String()
}

// mark is the character formatting of one run.
type mark struct {
	bold, italic, underline, strike bool
}

// inlineStyle is compared to know when to close one wrapper and open another.
type inlineStyle struct {
	link string
	m    mark
}

type seg struct {
	text string
	m    mark
	link string
	img  bool
	brk  bool
}

type para struct {
	segs   []seg
	style  string
	numID  string
	ilvl   int
	outline int // w:outlineLvl + 1, or 0 when the paragraph carries none
}

// block is one Markdown block; list items sit on consecutive lines.
type block struct {
	text string
	list bool
}

type tableBuilder struct {
	rows   [][]string
	row    []string
	cell   []string
	inCell bool
}

type numKey struct {
	id  string
	lvl int
}

type docReader struct {
	files     map[string]*zip.File
	rels      map[string]string
	numbering map[numKey]bool // numId + ilvl -> ordered (false means bullet)

	blocks []block

	paras   []*para
	inPPr   bool
	inNumPr bool
	inRun   bool
	inRPr   bool
	inText  bool
	marks   mark
	links   []string
	skip    int // depth inside <w:del>, whose content is dropped
	tables  []*tableBuilder
}

func (d *docReader) parse(dec *xml.Decoder) error {
	for {
		tok, err := dec.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("docx: %w", err)
		}
		switch el := tok.(type) {
		case xml.StartElement:
			if d.skip > 0 {
				if isWord(el.Name) && el.Name.Local == "del" {
					d.skip++
				}
				continue
			}
			if isWord(el.Name) && el.Name.Local == "del" {
				d.skip = 1
				continue
			}
			d.start(el.Name, el.Attr)
		case xml.EndElement:
			if d.skip > 0 {
				if isWord(el.Name) && el.Name.Local == "del" {
					d.skip--
				}
				continue
			}
			d.end(el.Name)
		case xml.CharData:
			if d.skip == 0 && d.inText {
				d.addText(string(el), d.marks, d.currentLink())
			}
		}
	}
	for len(d.paras) > 0 {
		d.flushPara()
	}
	return nil
}

func (d *docReader) start(name xml.Name, attrs []xml.Attr) {
	switch name.Local {
	case "blip":
		if id := attrValue(attrs, "embed"); id != "" {
			d.addImage(id)
		} else if id := attrValue(attrs, "link"); id != "" {
			d.addImage(id)
		}
		return
	case "imagedata":
		if id := attrValue(attrs, "id"); id != "" {
			d.addImage(id)
		}
		return
	}
	if !isWord(name) {
		return
	}
	switch name.Local {
	case "p":
		d.paras = append(d.paras, &para{})
	case "pPr":
		d.inPPr = true
	case "pStyle":
		if d.inPPr {
			if p := d.currentPara(); p != nil {
				p.style = attrValue(attrs, "val")
			}
		}
	case "outlineLvl":
		// The reliable heading signal: Word writes 0 for the top level.
		if d.inPPr {
			if p := d.currentPara(); p != nil {
				if n, err := strconv.Atoi(attrValue(attrs, "val")); err == nil && n >= 0 && n < 6 {
					p.outline = n + 1
				}
			}
		}
	case "numPr":
		d.inNumPr = true
	case "numId":
		if d.inNumPr {
			if p := d.currentPara(); p != nil {
				p.numID = attrValue(attrs, "val")
			}
		}
	case "ilvl":
		if d.inNumPr {
			if n, err := strconv.Atoi(attrValue(attrs, "val")); err == nil && n >= 0 {
				if p := d.currentPara(); p != nil {
					p.ilvl = n
				}
			}
		}
	case "r":
		d.inRun = true
		d.inRPr = false
		d.marks = mark{}
	case "rPr":
		if d.inRun {
			d.inRPr = true
		}
	case "b":
		if d.inRPr {
			d.marks.bold = onOff(attrs)
		}
	case "i":
		if d.inRPr {
			d.marks.italic = onOff(attrs)
		}
	case "u":
		if d.inRPr {
			d.marks.underline = onOff(attrs)
		}
	case "strike":
		if d.inRPr {
			d.marks.strike = onOff(attrs)
		}
	case "t":
		d.inText = true
	case "tab":
		d.addText("\t", d.marks, d.currentLink())
	case "br", "cr":
		if p := d.currentPara(); p != nil {
			p.segs = append(p.segs, seg{brk: true})
		}
	case "hyperlink":
		d.links = append(d.links, d.linkTarget(attrs))
	case "tbl":
		d.tables = append(d.tables, &tableBuilder{})
	case "tr":
		if t := d.currentTable(); t != nil {
			t.row = nil
		}
	case "tc":
		if t := d.currentTable(); t != nil {
			t.cell = nil
			t.inCell = true
		}
	}
}

func (d *docReader) end(name xml.Name) {
	if !isWord(name) {
		return
	}
	switch name.Local {
	case "p":
		d.flushPara()
	case "pPr":
		d.inPPr = false
		d.inNumPr = false
	case "numPr":
		d.inNumPr = false
	case "r":
		d.inRun = false
		d.inRPr = false
		d.marks = mark{}
	case "rPr":
		d.inRPr = false
	case "t":
		d.inText = false
	case "hyperlink":
		if n := len(d.links); n > 0 {
			d.links = d.links[:n-1]
		}
	case "tc":
		d.endCell()
	case "tr":
		d.endRow()
	case "tbl":
		d.endTable()
	}
}

func (d *docReader) currentPara() *para {
	if len(d.paras) == 0 {
		return nil
	}
	return d.paras[len(d.paras)-1]
}

func (d *docReader) currentTable() *tableBuilder {
	if len(d.tables) == 0 {
		return nil
	}
	return d.tables[len(d.tables)-1]
}

func (d *docReader) currentLink() string {
	if n := len(d.links); n > 0 {
		return d.links[n-1]
	}
	return ""
}

func (d *docReader) inCell() bool {
	return len(d.tables) > 0 && d.tables[len(d.tables)-1].inCell
}

func (d *docReader) addText(s string, m mark, link string) {
	if s == "" {
		return
	}
	p := d.currentPara()
	if p == nil {
		return
	}
	if n := len(p.segs); n > 0 {
		last := &p.segs[n-1]
		if !last.img && !last.brk && last.m == m && last.link == link {
			last.text += s
			return
		}
	}
	p.segs = append(p.segs, seg{text: s, m: m, link: link})
}

func (d *docReader) addImage(rid string) {
	target := d.rels[rid]
	if target == "" {
		return
	}
	target = strings.ReplaceAll(target, " ", "%20")
	p := d.currentPara()
	if p == nil {
		p = &para{}
		d.paras = append(d.paras, p)
	}
	if n := len(p.segs); n > 0 && p.segs[n-1].img && p.segs[n-1].text == target {
		return
	}
	p.segs = append(p.segs, seg{text: target, img: true})
}

func (d *docReader) flushPara() {
	if len(d.paras) == 0 {
		return
	}
	p := d.paras[len(d.paras)-1]
	d.paras = d.paras[:len(d.paras)-1]
	text := strings.Trim(renderPara(p), " \t")
	if strings.TrimSpace(text) == "" {
		return
	}
	isList := false
	if level := headingLevel(p.style); level > 0 {
		text = strings.Repeat("#", level) + " " + text
	} else if p.outline > 0 {
		text = strings.Repeat("#", p.outline) + " " + text
	} else if p.numID != "" && p.numID != "0" {
		marker := "-"
		if d.numbering[numKey{p.numID, p.ilvl}] {
			marker = "1."
		}
		text = strings.Repeat("  ", p.ilvl) + marker + " " + text
		isList = true
	}
	if d.inCell() {
		t := d.tables[len(d.tables)-1]
		t.cell = append(t.cell, text)
		return
	}
	d.blocks = append(d.blocks, block{text: text, list: isList})
}

func (d *docReader) endCell() {
	t := d.currentTable()
	if t == nil {
		return
	}
	t.row = append(t.row, cellMarkdown(t.cell))
	t.cell = nil
	t.inCell = false
}

func (d *docReader) endRow() {
	t := d.currentTable()
	if t == nil {
		return
	}
	t.rows = append(t.rows, t.row)
	t.row = nil
}

func (d *docReader) endTable() {
	if len(d.tables) == 0 {
		return
	}
	t := d.tables[len(d.tables)-1]
	d.tables = d.tables[:len(d.tables)-1]
	md := renderTable(t.rows)
	if md == "" {
		return
	}
	// A table nested in a cell stays inside that cell.
	if d.inCell() {
		parent := d.tables[len(d.tables)-1]
		parent.cell = append(parent.cell, md)
		return
	}
	d.blocks = append(d.blocks, block{text: md})
}

// renderPara writes one paragraph's runs, opening and closing wrappers only
// when the formatting changes.
func renderPara(p *para) string {
	var out strings.Builder
	var cur inlineStyle
	setStyle := func(next inlineStyle) {
		if next == cur {
			return
		}
		if cur.m.strike {
			out.WriteString("~~")
		}
		if cur.m.underline {
			out.WriteString("</u>")
		}
		if cur.m.italic {
			out.WriteString("*")
		}
		if cur.m.bold {
			out.WriteString("**")
		}
		if cur.link != "" {
			out.WriteString("](")
			out.WriteString(cur.link)
			out.WriteByte(')')
		}
		if next.link != "" {
			out.WriteByte('[')
		}
		if next.m.bold {
			out.WriteString("**")
		}
		if next.m.italic {
			out.WriteByte('*')
		}
		if next.m.underline {
			out.WriteString("<u>")
		}
		if next.m.strike {
			out.WriteString("~~")
		}
		cur = next
	}
	for _, s := range p.segs {
		if s.brk {
			setStyle(inlineStyle{})
			out.WriteString("  \n")
			continue
		}
		if s.img {
			setStyle(inlineStyle{})
			out.WriteString("![](")
			out.WriteString(s.text)
			out.WriteByte(')')
			continue
		}
		st := inlineStyle{link: s.link, m: s.m}
		if s.m == (mark{}) {
			setStyle(st)
			out.WriteString(s.text)
			continue
		}
		// Keep edge spaces outside the emphasis markers: "**bold **" does not
		// render as bold.
		head := len(s.text) - len(strings.TrimLeft(s.text, " \t"))
		tail := len(strings.TrimRight(s.text, " \t"))
		plain := inlineStyle{link: s.link}
		if head >= tail {
			setStyle(plain)
			out.WriteString(s.text)
			continue
		}
		if head > 0 {
			setStyle(plain)
			out.WriteString(s.text[:head])
		}
		setStyle(st)
		out.WriteString(s.text[head:tail])
		if tail < len(s.text) {
			setStyle(plain)
			out.WriteString(s.text[tail:])
		}
	}
	setStyle(inlineStyle{})
	return out.String()
}

// renderTable writes GFM pipe rows; the first row is the header.
func renderTable(rows [][]string) string {
	if len(rows) == 0 {
		return ""
	}
	width := 1
	for _, r := range rows {
		if len(r) > width {
			width = len(r)
		}
	}
	var out strings.Builder
	writeRow := func(r []string) {
		out.WriteByte('|')
		for i := 0; i < width; i++ {
			out.WriteString(" ")
			if i < len(r) {
				out.WriteString(r[i])
			}
			out.WriteString(" |")
		}
		out.WriteByte('\n')
	}
	writeRow(rows[0])
	out.WriteByte('|')
	for i := 0; i < width; i++ {
		out.WriteString(" --- |")
	}
	out.WriteByte('\n')
	for _, r := range rows[1:] {
		writeRow(r)
	}
	return strings.TrimRight(out.String(), "\n")
}

// cellMarkdown keeps a cell on one line: paragraphs and hard breaks become
// <br>, and pipes are escaped so they do not end the column.
func cellMarkdown(paras []string) string {
	parts := make([]string, 0, len(paras))
	for _, p := range paras {
		if strings.TrimSpace(p) == "" {
			continue
		}
		p = strings.ReplaceAll(p, "  \n", "<br>")
		p = strings.ReplaceAll(p, "\n", "<br>")
		parts = append(parts, p)
	}
	return strings.ReplaceAll(strings.Join(parts, "<br>"), "|", `\|`)
}

// headingLevel maps a Word style name to a Markdown heading level, or 0.
func headingLevel(style string) int {
	s := strings.Join(strings.Fields(strings.ToLower(style)), "")
	if s == "" {
		return 0
	}
	if s == "title" {
		return 1
	}
	for _, prefix := range []string{"heading", "标题"} {
		if strings.HasPrefix(s, prefix) {
			n, err := strconv.Atoi(strings.TrimPrefix(s, prefix))
			if err == nil && n > 0 {
				if n > 6 {
					n = 6
				}
				return n
			}
		}
	}
	return 0
}

func (d *docReader) loadRels(name string) error {
	f := d.files[name]
	if f == nil {
		return nil
	}
	rc, err := f.Open()
	if err != nil {
		return fmt.Errorf("docx: %w", err)
	}
	defer rc.Close()
	var root struct {
		Rels []struct {
			ID     string `xml:"Id,attr"`
			Target string `xml:"Target,attr"`
		} `xml:"Relationship"`
	}
	if err := xml.NewDecoder(rc).Decode(&root); err != nil {
		return fmt.Errorf("docx: %s: %w", name, err)
	}
	for _, r := range root.Rels {
		if r.ID != "" && r.Target != "" {
			d.rels[r.ID] = strings.ReplaceAll(r.Target, "\\", "/")
		}
	}
	return nil
}

func (d *docReader) loadNumbering(name string) {
	f := d.files[name]
	if f == nil {
		return
	}
	rc, err := f.Open()
	if err != nil {
		return
	}
	defer rc.Close()
	var root struct {
		Abstract []struct {
			ID     string `xml:"abstractNumId,attr"`
			Levels []struct {
				ILvl int `xml:"ilvl,attr"`
				Fmt  struct {
					Val string `xml:"val,attr"`
				} `xml:"numFmt"`
			} `xml:"lvl"`
		} `xml:"abstractNum"`
		Nums []struct {
			ID  string `xml:"numId,attr"`
			Abs struct {
				Val string `xml:"val,attr"`
			} `xml:"abstractNumId"`
		} `xml:"num"`
	}
	if err := xml.NewDecoder(rc).Decode(&root); err != nil {
		return
	}
	abstract := make(map[string]map[int]bool, len(root.Abstract))
	for _, a := range root.Abstract {
		levels := make(map[int]bool, len(a.Levels))
		for _, l := range a.Levels {
			levels[l.ILvl] = l.Fmt.Val != "" && l.Fmt.Val != "bullet" && l.Fmt.Val != "none"
		}
		abstract[a.ID] = levels
	}
	for _, n := range root.Nums {
		for lvl, ordered := range abstract[n.Abs.Val] {
			d.numbering[numKey{n.ID, lvl}] = ordered
		}
	}
}

func (d *docReader) linkTarget(attrs []xml.Attr) string {
	if id := attrValue(attrs, "id"); id != "" {
		if target := d.rels[id]; target != "" {
			return strings.ReplaceAll(target, " ", "%20")
		}
	}
	if anchor := attrValue(attrs, "anchor"); anchor != "" {
		return "#" + anchor
	}
	return ""
}

// attrValue reads an attribute by local name; namespace prefixes do not matter.
func attrValue(attrs []xml.Attr, local string) string {
	for _, a := range attrs {
		if a.Name.Local == local {
			return a.Value
		}
	}
	return ""
}

// onOff reads Word's tri-state toggle: absent means on, "0"/"false"/"off" off.
func onOff(attrs []xml.Attr) bool {
	switch strings.ToLower(attrValue(attrs, "val")) {
	case "0", "false", "off", "none":
		return false
	}
	return true
}

// isWord reports whether the element belongs to WordprocessingML. An empty
// namespace is accepted so hand-written documents without prefixes still parse.
func isWord(name xml.Name) bool {
	return name.Space == "" || strings.Contains(name.Space, "wordprocessingml")
}
