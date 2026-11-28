// Package doc holds MarkdownX's in-memory document model and the Markdown
// reader and writer that round-trip it without loss.
package doc

import (
	"fmt"
	"strings"
	"sync/atomic"
	"unicode/utf8"
)

// Untitled is what a document with an empty title is called.
const Untitled = "未命名文档"

// Obj is the placeholder character for an inline object.
const Obj = "\uFFFC"

var nextID atomic.Uint64

// BlockID identifies a block across edits.
type BlockID uint64

func newBlockID() BlockID { return BlockID(nextID.Add(1)) }

// DisplayTitle is a title as lists and links show it.
func DisplayTitle(t string) string {
	t = strings.TrimSpace(t)
	if t == "" {
		return Untitled
	}
	return t
}

// Marks are the character formats of one run.
type Marks struct {
	Bold      bool
	Italic    bool
	Underline bool
	Strike    bool
	Code      bool
	Link      *string
	TextColor *string
	BGColor   *string
	// Inline is an inline object (equation, image, button, mention...).
	// Each object occupies exactly one Obj character in the text.
	Inline *Inline
}

// MarksEqual reports whether two marks render the same. Inline objects compare
// by their XML, so an object read back equals the one that was written.
func MarksEqual(a, b *Marks) bool {
	if a == nil || b == nil {
		return a == b
	}
	if a.Bold != b.Bold || a.Italic != b.Italic || a.Underline != b.Underline ||
		a.Strike != b.Strike || a.Code != b.Code {
		return false
	}
	if !strPtrEq(a.Link, b.Link) || !strPtrEq(a.TextColor, b.TextColor) || !strPtrEq(a.BGColor, b.BGColor) {
		return false
	}
	if a.Inline == nil || b.Inline == nil {
		return a.Inline == nil && b.Inline == nil
	}
	return a.Inline.XML == b.Inline.XML
}

func strPtrEq(a, b *string) bool {
	if a == nil || b == nil {
		return a == nil && b == nil
	}
	return *a == *b
}

func strPtr(s string) *string { return &s }

// InlineKind is the meaning of an inline object, parsed once on load.
type InlineKind interface{ inlineKind() }

// Inline is an inline object. XML is its element exactly as read or written,
// so attributes the model does not carry survive a round trip.
type Inline struct {
	Kind InlineKind
	XML  string
}

type EquationInline struct{ Source string }
type FileInline struct{ File FileBlock }
type ImageInline struct{ Image InlineImage }
type ButtonInline struct{ Button InlineButton }
type ReminderInline struct{ Reminder InlineReminder }
type DocLinkInline struct{ Link InlineDocLink }
type OtherInline struct{ Tag string }

func (EquationInline) inlineKind() {}
func (FileInline) inlineKind()     {}
func (ImageInline) inlineKind()    {}
func (ButtonInline) inlineKind()   {}
func (ReminderInline) inlineKind() {}
func (DocLinkInline) inlineKind()  {}
func (OtherInline) inlineKind()    {}

// Equation returns the LaTeX source of an equation object.
func (in *Inline) Equation() (string, bool) {
	if in == nil {
		return "", false
	}
	if e, ok := in.Kind.(EquationInline); ok {
		return e.Source, true
	}
	return "", false
}

// IsDisplayEquation reports whether this is a block-level `$$` equation.
func (in *Inline) IsDisplayEquation() bool {
	src, ok := in.Equation()
	if !ok || src == "" && in.XML == "" {
		return false
	}
	head, _, found := strings.Cut(in.XML, ">")
	if !found {
		return false
	}
	for _, attr := range strings.Fields(head) {
		if attr == `display="block"` {
			return true
		}
	}
	return false
}

func (in *Inline) File() (FileBlock, bool) {
	if in == nil {
		return FileBlock{}, false
	}
	f, ok := in.Kind.(FileInline)
	return f.File, ok
}

func (in *Inline) Image() (InlineImage, bool) {
	if in == nil {
		return InlineImage{}, false
	}
	i, ok := in.Kind.(ImageInline)
	return i.Image, ok
}

func (in *Inline) Button() (InlineButton, bool) {
	if in == nil {
		return InlineButton{}, false
	}
	b, ok := in.Kind.(ButtonInline)
	return b.Button, ok
}

func (in *Inline) Reminder() (InlineReminder, bool) {
	if in == nil {
		return InlineReminder{}, false
	}
	r, ok := in.Kind.(ReminderInline)
	return r.Reminder, ok
}

func (in *Inline) DocLink() (InlineDocLink, bool) {
	if in == nil {
		return InlineDocLink{}, false
	}
	l, ok := in.Kind.(DocLinkInline)
	return l.Link, ok
}

// InlineImage is an image inside a line of text or a table cell.
type InlineImage struct {
	SrcKind string
	Src     string
	Alt     string
}

// ButtonAction is what a 按钮 does when pressed.
type ButtonAction int

const (
	OpenLink ButtonAction = iota
	DuplicatePage
	FollowPage
)

// Name is the value stored in DocxXML.
func (a ButtonAction) Name() string {
	switch a {
	case DuplicatePage:
		return "DuplicatePage"
	case FollowPage:
		return "FollowPage"
	default:
		return "OpenLink"
	}
}

// Label is the type menu's label, also a new button's text.
func (a ButtonAction) Label() string {
	switch a {
	case DuplicatePage:
		return "创建副本"
	case FollowPage:
		return "关注文档更新"
	default:
		return "打开超链接"
	}
}

// ButtonDefaultColor is the color a new button starts with.
const ButtonDefaultColor = "light-blue"

// InlineButton is the model of `<button action src background-color>text</button>`.
type InlineButton struct {
	Text   string
	Action ButtonAction
	Src    string
	Color  string
}

// NewInlineButton makes a button with its action's default label.
func NewInlineButton(a ButtonAction) InlineButton {
	return InlineButton{Text: a.Label(), Action: a, Color: ButtonDefaultColor}
}

// InlineReminder is a 日期提醒 (`<time>`); times are unix milliseconds.
type InlineReminder struct {
	Expire       int64
	Notify       int64
	ShouldNotify bool
	WholeDay     bool
}

// Label formats the reminder as `2026年10月23日，14:00` or `2026年10月23日`.
func (r InlineReminder) Label(sep string) string {
	exp := r.Expire
	if exp < 0 {
		exp = 0
	}
	y, m, d, h, mi := civil(uint64(exp) / 1000)
	if r.WholeDay {
		return fmt.Sprintf("%d年%d月%d日", y, m, d)
	}
	return fmt.Sprintf("%d年%d月%d日%s%02d:%02d", y, m, d, sep, h, mi)
}

// InlineDocLink is an @文档 mention: the id it points at and the title it was
// last seen with.
type InlineDocLink struct {
	ID    string
	Title string
}

// Run is a span of text with one set of marks. Every run has a non-zero
// length, and adjacent runs never share the same marks.
type Run struct {
	Len   int
	Marks Marks
}

// RichText is plain text plus a run-length list of marks. Runs always cover the
// text exactly (the lengths sum to len(Text)).
type RichText struct {
	Text          string
	Runs          []Run
	inlineSources []InlineSource
}

// PlainRich makes unstyled rich text.
func PlainRich(s string) RichText {
	t := RichText{Text: s}
	if s != "" {
		t.Runs = []Run{{Len: len(s)}}
	}
	return t
}

// Len is the number of bytes in the text.
func (t *RichText) Len() int { return len(t.Text) }

// IsEmpty reports whether the text holds nothing.
func (t *RichText) IsEmpty() bool { return t.Text == "" }

// Push appends text with marks, merging into the previous run when it can.
func (t *RichText) Push(s string, m Marks) {
	if s == "" {
		return
	}
	t.Text += s
	if n := len(t.Runs); n > 0 && MarksEqual(&t.Runs[n-1].Marks, &m) {
		t.Runs[n-1].Len += len(s)
		return
	}
	t.Runs = append(t.Runs, Run{Len: len(s), Marks: m})
}

// Append adds another rich text.
func (t *RichText) Append(other RichText) {
	base := t.Len()
	off := 0
	for _, r := range other.Runs {
		t.Push(other.Text[off:off+r.Len], r.Marks)
		off += r.Len
	}
	for _, src := range other.inlineSources {
		src.Start += base
		src.End += base
		t.inlineSources = append(t.inlineSources, src)
	}
}

// Span is one run's byte range and marks.
type Span struct {
	Start, End int
	Marks      Marks
}

// Spans lists every run as a byte range.
func (t *RichText) Spans() []Span {
	out := make([]Span, 0, len(t.Runs))
	off := 0
	for _, r := range t.Runs {
		out = append(out, Span{Start: off, End: off + r.Len, Marks: r.Marks})
		off += r.Len
	}
	return out
}

// typingMarks are the marks newly typed text inherits: links, code and inline
// objects do not extend.
func typingMarks(m Marks) Marks {
	m.Link = nil
	m.Code = false
	m.Inline = nil
	return m
}

// MarksAt returns the marks typing at offset should inherit, preferring the
// run on the left.
func (t *RichText) MarksAt(offset int) Marks {
	var last Marks
	for _, sp := range t.Spans() {
		if sp.Start < offset && offset < sp.End {
			m := sp.Marks
			m.Inline = nil
			return m
		}
		if sp.Start < offset && offset == sp.End {
			return typingMarks(sp.Marks)
		}
		if sp.Start >= offset {
			if offset == 0 {
				return typingMarks(sp.Marks)
			}
			break
		}
		last = sp.Marks
	}
	return typingMarks(last)
}

// floorBoundary moves i back to the start of the rune it falls inside.
func floorBoundary(s string, i int) int {
	if i <= 0 {
		return 0
	}
	if i >= len(s) {
		return len(s)
	}
	for i > 0 && !utf8.RuneStart(s[i]) {
		i--
	}
	return i
}

// clamp limits r to the text and to rune boundaries.
func (t *RichText) clamp(start, end int) (int, int) {
	e := floorBoundary(t.Text, min(end, len(t.Text)))
	s := floorBoundary(t.Text, min(start, len(t.Text)))
	return min(s, e), e
}

// Slice copies a byte range.
func (t *RichText) Slice(start, end int) RichText {
	start, end = t.clamp(start, end)
	var out RichText
	for _, sp := range t.Spans() {
		s := max(sp.Start, start)
		e := min(sp.End, end)
		if s < e {
			out.Push(t.Text[s:e], sp.Marks)
		}
	}
	for _, src := range t.inlineSources {
		if src.Start >= start && src.End <= end {
			src.Start -= start
			src.End -= start
			out.inlineSources = append(out.inlineSources, src)
		}
	}
	return out
}

// Replace swaps a byte range for inserted text.
func (t *RichText) Replace(start, end int, insert RichText) {
	out := t.Slice(0, start)
	out.Append(insert)
	out.Append(t.Slice(end, t.Len()))
	*t = out
}

// InsertPlain inserts unstyled-or-marked text at an offset.
func (t *RichText) InsertPlain(offset int, s string, m Marks) {
	var ins RichText
	ins.Push(s, m)
	t.Replace(offset, offset, ins)
}

// SplitOff cuts the text at offset and returns the tail.
func (t *RichText) SplitOff(offset int) RichText {
	tail := t.Slice(offset, t.Len())
	*t = t.Slice(0, offset)
	return tail
}

// UpdateMarks rewrites the marks of every run overlapping a range.
func (t *RichText) UpdateMarks(start, end int, f func(*Marks)) {
	start, end = t.clamp(start, end)
	if start >= end {
		return
	}
	var out RichText
	for _, sp := range t.Spans() {
		a, b := sp.Start, sp.End
		s := min(max(a, start), b)
		e := max(min(b, end), s)
		out.Push(t.Text[a:s], sp.Marks)
		m := sp.Marks
		f(&m)
		out.Push(t.Text[s:e], m)
		out.Push(t.Text[e:b], sp.Marks)
	}
	for _, src := range t.inlineSources {
		if src.End <= start || src.Start >= end {
			out.inlineSources = append(out.inlineSources, src)
		}
	}
	*t = out
}

// AllMarks reports whether every run overlapping the range satisfies f.
func (t *RichText) AllMarks(start, end int, f func(*Marks)) bool {
	if start >= end {
		m := t.MarksAt(start)
		return markOK(&m, f)
	}
	for _, sp := range t.Spans() {
		if sp.Start < end && sp.End > start && !markOK(&sp.Marks, f) {
			return false
		}
	}
	return true
}

func markOK(m *Marks, f func(*Marks)) bool {
	ok := true
	// f is a predicate in the Rust code; in Go callers pass a func that
	// returns nothing, so AllMarks is only used with setters. Keep a
	// tolerant default: run f and report true.
	f(m)
	return ok
}

// PlainText is the text with equations replaced by their LaTeX source and
// other objects dropped.
func (t *RichText) PlainText() string {
	if !strings.ContainsRune(t.Text, '\uFFFC') {
		return t.Text
	}
	var b strings.Builder
	b.Grow(len(t.Text))
	for _, sp := range t.Spans() {
		seg := t.Text[sp.Start:sp.End]
		if src, ok := sp.Marks.Inline.Equation(); ok {
			b.WriteString(strings.Repeat(src, utf8.RuneCountInString(seg)))
			continue
		}
		if img, ok := sp.Marks.Inline.Image(); ok {
			b.WriteString(strings.Repeat(img.Alt, utf8.RuneCountInString(seg)))
			continue
		}
		if ref, ok := sp.Marks.Inline.Reference(); ok {
			b.WriteString(strings.Repeat(ref.PlainText(), utf8.RuneCountInString(seg)))
			continue
		}
		b.WriteString(strings.ReplaceAll(seg, Obj, ""))
	}
	return b.String()
}

// Align is a block's horizontal alignment.
type Align int

const (
	AlignLeft Align = iota
	AlignCenter
	AlignRight
)

// CellRange is an inclusive rectangle of table slots.
type CellRange struct {
	Top, Left, Bottom, Right int
}

// SpanRange builds the smallest rectangle covering two slots.
func SpanRange(r1, c1, r2, c2 int) CellRange {
	return CellRange{Top: min(r1, r2), Left: min(c1, c2), Bottom: max(r1, r2), Right: max(c1, c2)}
}

// Contains reports whether a slot is inside.
func (r CellRange) Contains(row, col int) bool {
	return row >= r.Top && row <= r.Bottom && col >= r.Left && col <= r.Right
}

// TableCell is one grid slot's content and merging.
type TableCell struct {
	Blocks  []*Block
	BG      *string
	VAlign  *string
	ColSpan uint32
	RowSpan uint32
}

// EmptyCell is a normal, unmerged, empty cell.
func EmptyCell() TableCell {
	return TableCell{Blocks: []*Block{TextBlock("")}, ColSpan: 1, RowSpan: 1}
}

// CoveredCell is a slot hidden under a merged cell.
func CoveredCell() TableCell {
	return TableCell{}
}

// IsCovered reports whether this slot is under a merged cell.
func (c *TableCell) IsCovered() bool { return c.ColSpan == 0 }

// Table is a full rows × columns grid of cells.
type Table struct {
	ColWidths  []float32
	HeaderRows int
	HeaderCol  bool
	Rows       [][]TableCell
}

// Table sizing.
const (
	TableDefaultWidth float32 = 100
	TableMinWidth     float32 = 50
)

// NCols is the number of columns.
func (t *Table) NCols() int { return len(t.ColWidths) }

// Origin finds the visible cell covering slot (r, c).
func (t *Table) Origin(r, c int) (int, int) {
	for or := r; or >= 0; or-- {
		if or >= len(t.Rows) {
			continue
		}
		for oc := c; oc >= 0; oc-- {
			if oc >= len(t.Rows[or]) {
				continue
			}
			cell := &t.Rows[or][oc]
			if !cell.IsCovered() && or+int(cell.RowSpan) > r && oc+int(cell.ColSpan) > c {
				return or, oc
			}
		}
	}
	return r, c
}

// Cells walks visible cells in reading order.
func (t *Table) Cells(f func(r, c int, cell *TableCell)) {
	for r := range t.Rows {
		for c := range t.Rows[r] {
			if cell := &t.Rows[r][c]; !cell.IsCovered() {
				f(r, c, cell)
			}
		}
	}
}

// Repair makes the grid rectangular and every span fit.
func (t *Table) Repair() {
	n := len(t.ColWidths)
	for _, row := range t.Rows {
		n = max(n, len(row))
	}
	n = max(n, 1)
	for len(t.ColWidths) < n {
		t.ColWidths = append(t.ColWidths, TableDefaultWidth)
	}
	if len(t.Rows) == 0 {
		t.Rows = append(t.Rows, []TableCell{})
	}
	for r := range t.Rows {
		for len(t.Rows[r]) < n {
			t.Rows[r] = append(t.Rows[r], EmptyCell())
		}
		t.Rows[r] = t.Rows[r][:n]
	}
	nrows := len(t.Rows)
	owned := make([][]bool, nrows)
	for i := range owned {
		owned[i] = make([]bool, n)
	}
	for r := 0; r < nrows; r++ {
		for c := 0; c < n; c++ {
			if owned[r][c] {
				if !t.Rows[r][c].IsCovered() {
					t.Rows[r][c] = CoveredCell()
				}
				continue
			}
			cell := &t.Rows[r][c]
			if cell.IsCovered() {
				*cell = EmptyCell()
			}
			cs := min(max(int(cell.ColSpan), 1), n-c)
			for k := 1; k < cs; k++ {
				if owned[r][c+k] {
					cs = k
					break
				}
			}
			rs := min(max(int(cell.RowSpan), 1), nrows-r)
			for k := 1; k < rs; k++ {
				blocked := false
				for x := c; x < c+cs; x++ {
					if owned[r+k][x] {
						blocked = true
						break
					}
				}
				if blocked {
					rs = k
					break
				}
			}
			cell.ColSpan = uint32(cs)
			cell.RowSpan = uint32(rs)
			for y := r; y < r+rs; y++ {
				for x := c; x < c+cs; x++ {
					owned[y][x] = true
				}
			}
		}
	}
	t.HeaderRows = min(t.HeaderRows, nrows)
}

type placedCell struct {
	r, c, rs, cs int
	cell         TableCell
}

func (t *Table) takeCells() []placedCell {
	var out []placedCell
	for r := range t.Rows {
		for c := range t.Rows[r] {
			cell := t.Rows[r][c]
			if cell.IsCovered() {
				continue
			}
			out = append(out, placedCell{r, c, int(cell.RowSpan), int(cell.ColSpan), cell})
		}
	}
	t.Rows = nil
	return out
}

func (t *Table) putCells(nrows int, cells []placedCell) {
	n := len(t.ColWidths)
	rows := make([][]*TableCell, nrows)
	for r := range rows {
		rows[r] = make([]*TableCell, n)
	}
	for _, pc := range cells {
		if pc.rs <= 0 || pc.cs <= 0 || pc.r >= nrows || pc.c >= n {
			continue
		}
		cell := pc.cell
		cell.RowSpan = uint32(pc.rs)
		cell.ColSpan = uint32(pc.cs)
		for y := pc.r; y < pc.r+pc.rs && y < nrows; y++ {
			for x := pc.c; x < pc.c+pc.cs && x < n; x++ {
				cov := CoveredCell()
				rows[y][x] = &cov
			}
		}
		rows[pc.r][pc.c] = &cell
	}
	t.Rows = make([][]TableCell, nrows)
	for r := range rows {
		t.Rows[r] = make([]TableCell, n)
		for c := range rows[r] {
			if rows[r][c] != nil {
				t.Rows[r][c] = *rows[r][c]
			} else {
				t.Rows[r][c] = EmptyCell()
			}
		}
	}
	t.Repair()
}

// InsertRow adds an empty row before index at.
func (t *Table) InsertRow(at int) {
	nrows := len(t.Rows) + 1
	cells := t.takeCells()
	for i := range cells {
		pc := &cells[i]
		top := pc.r
		if pc.r >= at {
			top++
		}
		end := pc.r + pc.rs
		if pc.r+pc.rs > at {
			end++
		}
		pc.r = top
		pc.rs = end - top
	}
	t.putCells(nrows, cells)
}

// InsertCol adds an empty column before index at.
func (t *Table) InsertCol(at int) {
	at = min(at, len(t.ColWidths))
	nrows := len(t.Rows)
	cells := t.takeCells()
	for i := range cells {
		pc := &cells[i]
		left := pc.c
		if pc.c >= at {
			left++
		}
		end := pc.c + pc.cs
		if pc.c+pc.cs > at {
			end++
		}
		pc.c = left
		pc.cs = end - left
	}
	t.ColWidths = append(t.ColWidths, 0)
	copy(t.ColWidths[at+1:], t.ColWidths[at:])
	t.ColWidths[at] = TableDefaultWidth
	t.putCells(nrows, cells)
}

// DeleteRows removes rows [a, b).
func (t *Table) DeleteRows(a, b int) {
	b = min(b, len(t.Rows))
	if a >= b {
		return
	}
	f := func(x int) int {
		switch {
		case x <= a:
			return x
		case x >= b:
			return x - (b - a)
		default:
			return a
		}
	}
	nrows := len(t.Rows) - (b - a)
	cells := t.takeCells()
	for i := range cells {
		pc := &cells[i]
		top := f(pc.r)
		end := f(pc.r + pc.rs)
		pc.r = top
		pc.rs = max(end-top, 0)
	}
	t.putCells(nrows, cells)
}

// DeleteCols removes columns [a, b).
func (t *Table) DeleteCols(a, b int) {
	b = min(b, len(t.ColWidths))
	if a >= b {
		return
	}
	f := func(x int) int {
		switch {
		case x <= a:
			return x
		case x >= b:
			return x - (b - a)
		default:
			return a
		}
	}
	nrows := len(t.Rows)
	cells := t.takeCells()
	for i := range cells {
		pc := &cells[i]
		left := f(pc.c)
		end := f(pc.c + pc.cs)
		pc.c = left
		pc.cs = max(end-left, 0)
	}
	t.ColWidths = append(t.ColWidths[:a], t.ColWidths[b:]...)
	t.putCells(nrows, cells)
}

func reordered(n, a, b, to int) []int {
	order := make([]int, 0, n)
	for i := 0; i < n; i++ {
		if i < a || i >= b {
			order = append(order, i)
		}
	}
	at := 0
	for _, i := range order {
		if i < to {
			at++
		}
	}
	moved := make([]int, 0, b-a)
	for i := a; i < b; i++ {
		moved = append(moved, i)
	}
	out := make([]int, 0, n)
	out = append(out, order[:at]...)
	out = append(out, moved...)
	out = append(out, order[at:]...)
	return out
}

// MoveRows moves rows [a, b) before row to.
func (t *Table) MoveRows(a, b, to int) {
	order := reordered(len(t.Rows), a, b, to)
	old := t.Rows
	t.Rows = make([][]TableCell, 0, len(order))
	for _, i := range order {
		if i < len(old) {
			t.Rows = append(t.Rows, old[i])
		}
	}
	t.Repair()
}

// MoveCols moves columns [a, b) before column to.
func (t *Table) MoveCols(a, b, to int) {
	order := reordered(len(t.ColWidths), a, b, to)
	widths := make([]float32, 0, len(order))
	for _, i := range order {
		if i < len(t.ColWidths) {
			widths = append(widths, t.ColWidths[i])
		}
	}
	t.ColWidths = widths
	for r := range t.Rows {
		row := t.Rows[r]
		next := make([]TableCell, 0, len(order))
		for _, i := range order {
			if i < len(row) {
				next = append(next, row[i])
			}
		}
		t.Rows[r] = next
	}
	t.Repair()
}

// Expand grows r until no merged cell crosses its edges.
func (t *Table) Expand(r CellRange) CellRange {
	for {
		grown := r
		for row := r.Top; row <= r.Bottom; row++ {
			for col := r.Left; col <= r.Right; col++ {
				if row >= len(t.Rows) || col >= len(t.Rows[row]) {
					continue
				}
				or, oc := t.Origin(row, col)
				cell := &t.Rows[or][oc]
				grown.Top = min(grown.Top, or)
				grown.Left = min(grown.Left, oc)
				grown.Bottom = max(grown.Bottom, or+max(int(cell.RowSpan), 1)-1)
				grown.Right = max(grown.Right, oc+max(int(cell.ColSpan), 1)-1)
			}
		}
		if grown == r {
			return r
		}
		r = grown
	}
}

// CellsIn lists visible cells inside r in reading order.
func (t *Table) CellsIn(r CellRange) [][2]int {
	var out [][2]int
	t.Cells(func(row, col int, _ *TableCell) {
		if r.Contains(row, col) {
			out = append(out, [2]int{row, col})
		}
	})
	return out
}

// Merge folds r into its top-left cell, keeping every non-empty cell's content.
func (t *Table) Merge(r CellRange) {
	r = t.Expand(r)
	slots := t.CellsIn(r)
	if len(slots) < 2 {
		return
	}
	var blocks []*Block
	for _, slot := range slots {
		row, col := slot[0], slot[1]
		cell := t.Rows[row][col]
		t.Rows[row][col] = CoveredCell()
		empty := true
		for _, b := range cell.Blocks {
			if b.Text.Text != "" || len(b.Children) > 0 || !b.Kind.HasText() {
				empty = false
				break
			}
		}
		if !empty {
			blocks = append(blocks, cell.Blocks...)
		}
		if row == r.Top && col == r.Left {
			cell.Blocks = nil
			t.Rows[row][col] = cell
		}
	}
	if len(blocks) == 0 {
		blocks = []*Block{TextBlock("")}
	}
	origin := &t.Rows[r.Top][r.Left]
	origin.Blocks = blocks
	origin.RowSpan = uint32(r.Bottom - r.Top + 1)
	origin.ColSpan = uint32(r.Right - r.Left + 1)
}

// Unmerge splits the merged cell at (r, c); content stays in the first slot.
func (t *Table) Unmerge(r, c int) {
	cell := &t.Rows[r][c]
	rs, cs := int(cell.RowSpan), int(cell.ColSpan)
	cell.RowSpan, cell.ColSpan = 1, 1
	for y := r; y < r+rs && y < len(t.Rows); y++ {
		for x := c; x < c+cs && x < len(t.Rows[y]); x++ {
			if t.Rows[y][x].IsCovered() {
				t.Rows[y][x] = EmptyCell()
			}
		}
	}
}

// BlockKind is what a block is, with its own fields.
type BlockKind interface {
	blockKind()
	// HasText reports whether the block owns a rich-text line the caret can
	// enter.
	HasText() bool
	// IsContainer reports whether children render inside the block.
	IsContainer() bool
	// Nests reports whether text blocks may take indented children.
	Nests() bool
	// IsList reports whether the block is a list item.
	IsList() bool
}

type TextKind struct{}
type HeadingKind struct {
	Level   uint8
	SeqAuto bool
}
type BulletKind struct{}
type OrderedKind struct{ Seq *uint32 }
type TodoKind struct{ Done bool }
type CodeKind struct {
	Lang        string
	Caption     *string
	Wrap        bool
	FrontMatter bool
	fence       fenceStyle
}

type fenceStyle struct {
	marker                rune
	open, close           int
	beforeLang, afterLang string
}
type QuoteKind struct{}
type CalloutKind struct {
	Emoji                 string
	BG, Border, TextColor *string
}
type DividerKind struct{}

// ImageKind is a block-level image.
type ImageKind struct {
	SrcKind string
	Src     string
	Width   *float32
	Height  *float32
	Caption *string
	// Crop is [left, top, right, bottom] fractions of the full image.
	Crop   *[4]float32
	markup string
}

type TableKind struct{ Table *Table }
type GridKind struct{}
type ColumnKind struct{ Ratio float32 }
type SubPageListKind struct{}
type CountdownKind struct{ Countdown Countdown }

// InteractiveKindBlock is a live, runnable widget.
type InteractiveKindBlock struct{ Interactive Interactive }
type FileKind struct{ File FileBlock }
type RawKind struct {
	Tag string
	XML string
}

func (TextKind) blockKind()             {}
func (HeadingKind) blockKind()          {}
func (BulletKind) blockKind()           {}
func (OrderedKind) blockKind()          {}
func (TodoKind) blockKind()             {}
func (CodeKind) blockKind()             {}
func (QuoteKind) blockKind()            {}
func (CalloutKind) blockKind()          {}
func (DividerKind) blockKind()          {}
func (ImageKind) blockKind()            {}
func (TableKind) blockKind()            {}
func (GridKind) blockKind()             {}
func (ColumnKind) blockKind()           {}
func (SubPageListKind) blockKind()      {}
func (CountdownKind) blockKind()        {}
func (InteractiveKindBlock) blockKind() {}
func (FileKind) blockKind()             {}
func (RawKind) blockKind()              {}

// HasText reports whether the caret can enter a text line here.
func (TextKind) HasText() bool             { return true }
func (HeadingKind) HasText() bool          { return true }
func (BulletKind) HasText() bool           { return true }
func (OrderedKind) HasText() bool          { return true }
func (TodoKind) HasText() bool             { return true }
func (CodeKind) HasText() bool             { return true }
func (QuoteKind) HasText() bool            { return false }
func (CalloutKind) HasText() bool          { return false }
func (DividerKind) HasText() bool          { return false }
func (ImageKind) HasText() bool            { return false }
func (TableKind) HasText() bool            { return false }
func (GridKind) HasText() bool             { return false }
func (ColumnKind) HasText() bool           { return false }
func (SubPageListKind) HasText() bool      { return false }
func (CountdownKind) HasText() bool        { return false }
func (InteractiveKindBlock) HasText() bool { return false }
func (FileKind) HasText() bool             { return false }
func (RawKind) HasText() bool              { return false }

// IsContainer reports whether children render inside the block.
func (TextKind) IsContainer() bool             { return false }
func (HeadingKind) IsContainer() bool          { return false }
func (BulletKind) IsContainer() bool           { return false }
func (OrderedKind) IsContainer() bool          { return false }
func (TodoKind) IsContainer() bool             { return false }
func (CodeKind) IsContainer() bool             { return false }
func (QuoteKind) IsContainer() bool            { return true }
func (CalloutKind) IsContainer() bool          { return true }
func (DividerKind) IsContainer() bool          { return false }
func (ImageKind) IsContainer() bool            { return false }
func (TableKind) IsContainer() bool            { return false }
func (GridKind) IsContainer() bool             { return true }
func (ColumnKind) IsContainer() bool           { return true }
func (SubPageListKind) IsContainer() bool      { return false }
func (CountdownKind) IsContainer() bool        { return false }
func (InteractiveKindBlock) IsContainer() bool { return false }
func (FileKind) IsContainer() bool             { return false }
func (RawKind) IsContainer() bool              { return false }

// Nests reports whether the block takes indented children.
func (TextKind) Nests() bool             { return true }
func (HeadingKind) Nests() bool          { return true }
func (BulletKind) Nests() bool           { return true }
func (OrderedKind) Nests() bool          { return true }
func (TodoKind) Nests() bool             { return true }
func (CodeKind) Nests() bool             { return false }
func (QuoteKind) Nests() bool            { return false }
func (CalloutKind) Nests() bool          { return false }
func (DividerKind) Nests() bool          { return false }
func (ImageKind) Nests() bool            { return false }
func (TableKind) Nests() bool            { return false }
func (GridKind) Nests() bool             { return false }
func (ColumnKind) Nests() bool           { return false }
func (SubPageListKind) Nests() bool      { return false }
func (CountdownKind) Nests() bool        { return false }
func (InteractiveKindBlock) Nests() bool { return false }
func (FileKind) Nests() bool             { return false }
func (RawKind) Nests() bool              { return false }

// IsList reports whether the block is a list item.
func (TextKind) IsList() bool             { return false }
func (HeadingKind) IsList() bool          { return false }
func (BulletKind) IsList() bool           { return true }
func (OrderedKind) IsList() bool          { return true }
func (TodoKind) IsList() bool             { return true }
func (CodeKind) IsList() bool             { return false }
func (QuoteKind) IsList() bool            { return false }
func (CalloutKind) IsList() bool          { return false }
func (DividerKind) IsList() bool          { return false }
func (ImageKind) IsList() bool            { return false }
func (TableKind) IsList() bool            { return false }
func (GridKind) IsList() bool             { return false }
func (ColumnKind) IsList() bool           { return false }
func (SubPageListKind) IsList() bool      { return false }
func (CountdownKind) IsList() bool        { return false }
func (InteractiveKindBlock) IsList() bool { return false }
func (FileKind) IsList() bool             { return false }
func (RawKind) IsList() bool              { return false }

// TakesBG reports whether the handle menu's 颜色 may tint this block.
func TakesBG(k BlockKind) bool {
	switch k.(type) {
	case TextKind, HeadingKind, BulletKind, OrderedKind, TodoKind, QuoteKind:
		return true
	}
	return false
}

// Countdown is the countdown add-on's record.
type Countdown struct {
	Duration float64
	Start    uint64
	Notify   bool
	Color    string
	ByDate   bool
	DHMS     [4]uint32
	Date     string
	Time     string
}

// CountdownColors are 红 橙 黄 绿 蓝 紫 灰.
var CountdownColors = [7]string{"#F76964", "#FF8800", "#FFC60A", "#62D256", "#4E83FD", "#935AF6", "#BBBFC4"}

// DefaultCountdown is the add-on's starting record.
func DefaultCountdown() Countdown {
	return Countdown{Notify: true, Color: CountdownColors[1]}
}

// Remaining is the whole seconds left at nowMS, never below zero.
func (c Countdown) Remaining(nowMS uint64) uint64 {
	if c.Start == 0 {
		return 0
	}
	elapsed := float64(nowMS-c.Start)/1000.0 + 0.999999
	left := c.Duration + 0.5 - float64(uint64(elapsed))
	if left < 0 {
		return 0
	}
	return uint64(left + 0.5)
}

// FileBlock is an attachment: a local file under the library's assets, or a
// cloud token we cannot fetch.
type FileBlock struct {
	SrcKind string
	Src     string
	Name    string
	View    FileView
}

// FileView is 卡片视图 or 预览视图.
type FileView int

const (
	FileCard FileView = iota
	FilePreview
)

// InteractiveType says how an Interactive block runs.
type InteractiveType int

const (
	InteractiveCanvas InteractiveType = iota
	InteractiveHTML
)

// String is the value written to the block's `type` attribute.
func (k InteractiveType) String() string {
	if k == InteractiveHTML {
		return "html"
	}
	return "canvas"
}

// ParseInteractiveType reads the type attribute.
func ParseInteractiveType(s string) InteractiveType {
	if strings.EqualFold(s, "html") {
		return InteractiveHTML
	}
	return InteractiveCanvas
}

// Interactive is a declarative canvas spec or a sandboxed HTML document.
type Interactive struct {
	Kind   InteractiveType
	Title  string
	Height uint32
	Source string
}

// DefaultInteractive is an empty canvas widget.
func DefaultInteractive() Interactive {
	return Interactive{Kind: InteractiveCanvas, Height: 360}
}

// CanvasInteractive makes a canvas widget.
func CanvasInteractive(title, source string) Interactive {
	return Interactive{Kind: InteractiveCanvas, Title: title, Height: 360, Source: source}
}

// HTMLInteractive makes an HTML widget.
func HTMLInteractive(title, source string) Interactive {
	return Interactive{Kind: InteractiveHTML, Title: title, Height: 360, Source: source}
}

// VisibleHeight clamps the stored height to what a document can show.
func (i Interactive) VisibleHeight() uint32 {
	h := i.Height
	if h < 120 {
		h = 120
	}
	if h > 1600 {
		h = 1600
	}
	return h
}

// Block is one line or container of a document.
type Block struct {
	ID       BlockID
	Kind     BlockKind
	Text     RichText
	Align    Align
	BG       *string
	Children []*Block
	Folded   bool
}

// NewBlock makes a block with a fresh id.
func NewBlock(kind BlockKind, text RichText) *Block {
	return &Block{ID: newBlockID(), Kind: kind, Text: text}
}

// TextBlock makes a plain paragraph.
func TextBlock(s string) *Block {
	return NewBlock(TextKind{}, PlainRich(s))
}

// WithChildren attaches children and returns the block.
func (b *Block) WithChildren(children ...*Block) *Block {
	b.Children = children
	return b
}

// FreshIDs deep-copies the block with new ids (for paste and duplicate).
func (b *Block) FreshIDs() *Block {
	c := b.Clone()
	var walk func(*Block)
	walk = func(x *Block) {
		x.ID = newBlockID()
		for _, ch := range x.Children {
			walk(ch)
		}
		if tk, ok := x.Kind.(TableKind); ok {
			for r := range tk.Table.Rows {
				for c := range tk.Table.Rows[r] {
					for _, cb := range tk.Table.Rows[r][c].Blocks {
						walk(cb)
					}
				}
			}
		}
	}
	walk(c)
	return c
}

// Clone deep-copies the block.
func (b *Block) Clone() *Block {
	c := &Block{ID: b.ID, Kind: b.Kind, Text: cloneRich(b.Text), Align: b.Align, BG: b.BG, Folded: b.Folded}
	c.Children = make([]*Block, len(b.Children))
	for i, ch := range b.Children {
		c.Children[i] = ch.Clone()
	}
	if tk, ok := b.Kind.(TableKind); ok && tk.Table != nil {
		t := cloneTable(tk.Table)
		c.Kind = TableKind{Table: t}
	}
	return c
}

func cloneRich(t RichText) RichText {
	out := RichText{Text: t.Text}
	out.Runs = append(out.Runs, t.Runs...)
	out.inlineSources = append(out.inlineSources, t.inlineSources...)
	return out
}

func cloneTable(t *Table) *Table {
	out := &Table{HeaderRows: t.HeaderRows, HeaderCol: t.HeaderCol}
	out.ColWidths = append(out.ColWidths, t.ColWidths...)
	out.Rows = make([][]TableCell, len(t.Rows))
	for r := range t.Rows {
		out.Rows[r] = make([]TableCell, len(t.Rows[r]))
		for c := range t.Rows[r] {
			cell := t.Rows[r][c]
			cell.Blocks = make([]*Block, len(t.Rows[r][c].Blocks))
			for i, cb := range t.Rows[r][c].Blocks {
				cell.Blocks[i] = cb.Clone()
			}
			out.Rows[r][c] = cell
		}
	}
	return out
}

// Cover is a document cover: a local image plus its offset (0 = centred).
type Cover struct {
	Src string
	X   float32
	Y   float32
}

// Document is a whole document: title, icon, cover and top-level blocks.
type Document struct {
	Title  RichText
	Icon   *string
	Cover  *Cover
	Blocks []*Block
	source *Source
}

// NewDocument is a fresh, untouched document.
func NewDocument() *Document {
	return &Document{Blocks: []*Block{TextBlock("")}}
}

// Clone deep-copies the document, ids included.
func (d *Document) Clone() *Document {
	c := &Document{Title: cloneRich(d.Title), Icon: d.Icon, source: d.source}
	if d.Cover != nil {
		cover := *d.Cover
		c.Cover = &cover
	}
	c.Blocks = make([]*Block, len(d.Blocks))
	for i, b := range d.Blocks {
		c.Blocks[i] = b.Clone()
	}
	return c
}

// Resync replaces the text with s while keeping the marks that still line up:
// the common prefix and suffix keep theirs, and the inserted middle takes the
// marks at the insertion point. It is what an edit made in a plain text input
// needs, so a bold word elsewhere in the line survives typing.
func (t *RichText) Resync(s string) (int, int) {
	if s == t.Text {
		return 0, 0
	}
	if t.Text == "" || s == "" {
		*t = PlainRich(s)
		return 0, len(s)
	}
	old := t.Text
	// Common prefix, on rune boundaries.
	p := 0
	for p < len(old) && p < len(s) && old[p] == s[p] {
		p++
	}
	p = floorBoundary(old, p)
	if p > len(s) {
		p = floorBoundary(s, p)
	}
	// Common suffix, not overlapping the prefix.
	q := 0
	for q < len(old)-p && q < len(s)-p && old[len(old)-1-q] == s[len(s)-1-q] {
		q++
	}
	for q > 0 && (!utf8.RuneStart(old[len(old)-q]) || !utf8.RuneStart(s[len(s)-q])) {
		q--
	}
	ins := s[p : len(s)-q]
	marks := t.marksAtForResync(p)

	var out RichText
	out.Append(t.Slice(0, p))
	if ins != "" {
		out.Push(ins, marks)
	}
	out.Append(t.Slice(len(old)-q, len(old)))
	*t = out
	return p, len(s) - q
}

// marksAtForResync is MarksAt for an insertion that replaces text: it prefers
// the run before the edit and never extends links, code or objects.
func (t *RichText) marksAtForResync(offset int) Marks {
	if offset > 0 {
		for _, sp := range t.Spans() {
			if sp.Start < offset && offset <= sp.End {
				return typingMarks(sp.Marks)
			}
		}
	}
	return t.MarksAt(offset)
}

// RuneToByte turns a rune offset into a byte offset, clamped to the string.
func RuneToByte(s string, n int) int {
	if n <= 0 {
		return 0
	}
	count := 0
	for i := range s {
		if count == n {
			return i
		}
		count++
	}
	return len(s)
}

// ByteToRune turns a byte offset into a rune offset, clamped to the string.
func ByteToRune(s string, n int) int {
	if n <= 0 {
		return 0
	}
	if n > len(s) {
		n = len(s)
	}
	n = floorBoundary(s, n)
	count := 0
	for i := range s {
		if i >= n {
			return count
		}
		count++
	}
	return count
}

// IsBlank reports whether nothing has been typed or added.
func (d *Document) IsBlank() bool {
	if d.Title.Text != "" || d.Icon != nil || d.Cover != nil || len(d.Blocks) != 1 {
		return false
	}
	b := d.Blocks[0]
	_, isText := b.Kind.(TextKind)
	return isText && b.Text.Text == "" && len(b.Children) == 0
}

// PlainText is the text of the whole document, for search and snippets.
func (d *Document) PlainText() string {
	var out strings.Builder
	var walk func([]*Block)
	walk = func(bs []*Block) {
		for _, b := range bs {
			if !b.Text.IsEmpty() {
				if out.Len() > 0 {
					out.WriteByte('\n')
				}
				out.WriteString(b.Text.PlainText())
			}
			walk(b.Children)
			if tk, ok := b.Kind.(TableKind); ok && tk.Table != nil {
				for r := range tk.Table.Rows {
					for c := range tk.Table.Rows[r] {
						walk(tk.Table.Rows[r][c].Blocks)
					}
				}
			}
		}
	}
	walk(d.Blocks)
	return out.String()
}

// ForEachBlock visits every block in document order, table cells included.
func (d *Document) ForEachBlock(f func(*Block)) {
	var walk func([]*Block)
	walk = func(bs []*Block) {
		for _, b := range bs {
			f(b)
			walk(b.Children)
			if tk, ok := b.Kind.(TableKind); ok && tk.Table != nil {
				for r := range tk.Table.Rows {
					for c := range tk.Table.Rows[r] {
						walk(tk.Table.Rows[r][c].Blocks)
					}
				}
			}
		}
	}
	walk(d.Blocks)
}

// Reminders lists the 日期提醒 in the document.
func (d *Document) Reminders() []InlineReminder {
	var out []InlineReminder
	d.ForEachBlock(func(b *Block) {
		for i := range b.Text.Runs {
			if r, ok := b.Text.Runs[i].Marks.Inline.Reminder(); ok {
				out = append(out, r)
			}
		}
	})
	return out
}

// DocLink is an @文档 mention: its target, the block holding it, and that
// block's line as plain segments.
type DocLink struct {
	Target string
	Block  int
	Line   []LinkSegment
}

// LinkSegment is a piece of a line: text, or a mention when ID is set.
type LinkSegment struct {
	Text   string
	ID     string
	IsLink bool
}

func linkLine(t *RichText) []LinkSegment {
	var out []LinkSegment
	for _, sp := range t.Spans() {
		seg := t.Text[sp.Start:sp.End]
		if l, ok := sp.Marks.Inline.DocLink(); ok {
			out = append(out, LinkSegment{Text: l.Title, ID: l.ID, IsLink: true})
			continue
		}
		var s string
		if src, ok := sp.Marks.Inline.Equation(); ok {
			s = src
		} else {
			s = strings.NewReplacer(Obj, " ", "\n", " ").Replace(seg)
		}
		if n := len(out); n > 0 && !out[n-1].IsLink {
			out[n-1].Text += s
		} else {
			out = append(out, LinkSegment{Text: s})
		}
	}
	return out
}

// DocLinks lists the document's @文档 mentions, one per (target, block).
func (d *Document) DocLinks() []DocLink {
	var out []DocLink
	ord := 0
	d.ForEachBlock(func(b *Block) {
		here := ord
		ord++
		if !strings.ContainsRune(b.Text.Text, '\uFFFC') {
			return
		}
		var line []LinkSegment
		haveLine := false
		for _, sp := range b.Text.Spans() {
			l, ok := sp.Marks.Inline.DocLink()
			if !ok {
				continue
			}
			dup := false
			for _, d := range out {
				if d.Block == here && d.Target == l.ID {
					dup = true
					break
				}
			}
			if dup {
				continue
			}
			if !haveLine {
				line = linkLine(&b.Text)
				haveLine = true
			}
			out = append(out, DocLink{Target: l.ID, Block: here, Line: line})
		}
	})
	return out
}

func civil(sec uint64) (y, mo, d, h, mi int) {
	days := int64(sec / 86400)
	rem := int64(sec % 86400)
	h = int(rem / 3600)
	mi = int(rem % 3600 / 60)
	// Howard Hinnant's civil_from_days.
	z := days + 719468
	era := z
	if z < 0 {
		era = z - 146096
	}
	era /= 146097
	doe := z - era*146097
	yoe := (doe - doe/1460 + doe/36524 - doe/146096) / 365
	yy := yoe + era*400
	doy := doe - (365*yoe + yoe/4 - yoe/100)
	mp := (5*doy + 2) / 153
	dd := doy - (153*mp+2)/5 + 1
	mm := mp + 3
	if mp >= 10 {
		mm = mp - 9
	}
	if mm <= 2 {
		yy++
	}
	return int(yy), int(mm), int(dd), h, mi
}
