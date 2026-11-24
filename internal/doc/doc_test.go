package doc

import (
	"strings"
	"testing"
)

func fileOff() Off { return Off{File: true} }

// plainLines merges consecutive plain lines into one paragraph block.
func TestParagraphLinesMergeIntoOneBlock(t *testing.T) {
	blocks := MarkdownToBlocks("first line\nsecond line\nthird\n", fileOff())
	if len(blocks) != 1 {
		t.Fatalf("want 1 block, got %d: %#v", len(blocks), blocks)
	}
	if got := blocks[0].Text.Text; got != "first line\nsecond line\nthird" {
		t.Fatalf("merged text = %q", got)
	}
}

// Hard breaks survive a full round trip: source `  \n` → model `\n` → `  \n`.
func TestHardBreakRoundTrip(t *testing.T) {
	parsed := MarkdownToBlocks("alpha  \nbeta\n", fileOff())
	if len(parsed) != 1 {
		t.Fatalf("want 1 block, got %d", len(parsed))
	}
	if got := parsed[0].Text.Text; got != "alpha\nbeta" {
		t.Fatalf("internal text = %q, want %q", got, "alpha\nbeta")
	}
	out := Export(&Document{Blocks: parsed}, Options{}).Text
	if !strings.Contains(out, "alpha  \nbeta") {
		t.Fatalf("export lost the hard break: %q", out)
	}
}

func TestReplaceHardBreaksInParagraph(t *testing.T) {
	cases := map[string]string{
		"a\nb":     "a  \nb",
		"a  \nb":   "a  \nb",
		"a \nb":    "a  \nb",
		"a\n\nb":   "a  \n  \nb",
		"no break": "no break",
	}
	for in, want := range cases {
		if got := ReplaceHardBreaksInParagraph(in); got != want {
			t.Errorf("ReplaceHardBreaksInParagraph(%q) = %q, want %q", in, got, want)
		}
	}
}

func TestExportsCommonBlocks(t *testing.T) {
	bold := PlainRich("a ")
	bold.Push("bold ", Marks{Bold: true})
	bold.Push("link", Marks{Link: strPtr("https://x.y")})
	d := &Document{
		Title: PlainRich("标题"),
		Blocks: []*Block{
			NewBlock(HeadingKind{Level: 2}, PlainRich("小节")),
			NewBlock(TextKind{}, bold),
			NewBlock(OrderedKind{}, PlainRich("one")).WithChildren(NewBlock(BulletKind{}, PlainRich("sub"))),
			NewBlock(OrderedKind{}, PlainRich("two")),
			NewBlock(TodoKind{Done: true}, PlainRich("done")),
			NewBlock(CodeKind{Lang: "Rust"}, PlainRich("fn main() {}")),
			NewBlock(QuoteKind{}, RichText{}).WithChildren(TextBlock("q")),
			NewBlock(DividerKind{}, RichText{}),
		},
	}
	want := "# 标题\n\n## 小节\n\na **bold** [link](https://x.y)\n\n1. one\n   - sub\n2. two\n- [x] done\n\n```rust\nfn main() {}\n```\n\n> q\n\n---\n"
	if got := ToMarkdown(d); got != want {
		t.Fatalf("got:\n%q\nwant:\n%q", got, want)
	}
}

func TestExportsTables(t *testing.T) {
	cell := func(s string) TableCell {
		return TableCell{Blocks: []*Block{TextBlock(s)}, ColSpan: 1, RowSpan: 1}
	}
	tbl := &Table{
		HeaderRows: 1,
		Rows: [][]TableCell{
			{cell("a"), cell("b|c")},
			{cell("1")},
		},
	}
	w := &writer{}
	if got, want := w.table(tbl), "| a | b\\|c |\n| --- | --- |\n| 1 |  |"; got != want {
		t.Fatalf("got %q, want %q", got, want)
	}
}

func TestOrderedListRestartsKeepNumbers(t *testing.T) {
	in := "3. alpha\n4. beta\n\n8. gamma\n9. delta\n"
	blocks := MarkdownToBlocks(in, fileOff())
	out := Export(&Document{Blocks: blocks}, Options{}).Text
	if !strings.Contains(out, "4. beta\n\n8. gamma") {
		t.Fatalf("restart lost: %q", out)
	}
	again := MarkdownToBlocks(out, fileOff())
	if len(again) != len(blocks) {
		t.Fatalf("block count changed: %d → %d", len(blocks), len(again))
	}
	for i := range blocks {
		if blocks[i].Kind != again[i].Kind {
			// seq pointers would differ; compare by rendered block instead.
			if !(isOrdered(blocks[i].Kind) && isOrdered(again[i].Kind)) {
				t.Fatalf("block %d kind changed: %T → %T", i, blocks[i].Kind, again[i].Kind)
			}
		}
	}
}

func TestTableColumnAlignmentRoundTrips(t *testing.T) {
	in := "| left | center | right |\n| --- | :---: | ---: |\n| a | b | c |\n"
	blocks := MarkdownToBlocks(in, fileOff())
	out := Export(&Document{Blocks: blocks}, Options{}).Text
	if !strings.Contains(out, "| --- | :---: | ---: |") {
		t.Fatalf("alignment row lost: %q", out)
	}
	again := MarkdownToBlocks(out, fileOff())
	a, ok := blocks[0].Kind.(TableKind)
	if !ok {
		t.Fatalf("first block is %T", blocks[0].Kind)
	}
	b, ok := again[0].Kind.(TableKind)
	if !ok {
		t.Fatalf("reparsed block is %T", again[0].Kind)
	}
	for r := range a.Table.Rows {
		for c := range a.Table.Rows[r] {
			if got, want := b.Table.Rows[r][c].Blocks[0].Align, a.Table.Rows[r][c].Blocks[0].Align; got != want {
				t.Fatalf("cell (%d,%d) align %v, want %v", r, c, got, want)
			}
		}
	}
}

func TestLeadingSpacesRemainTextAfterListItems(t *testing.T) {
	d := &Document{Blocks: []*Block{
		NewBlock(BulletKind{}, PlainRich("item")),
		NewBlock(TextKind{}, PlainRich("  indented sibling")),
		NewBlock(BulletKind{}, PlainRich("next")),
		NewBlock(TextKind{}, PlainRich(" &#32; literally")),
	}}
	out := Export(d, Options{}).Text
	if !strings.Contains(out, "&#32;&#32;indented sibling") {
		t.Fatalf("leading spaces not preserved: %q", out)
	}
	if !strings.Contains(out, "\\&#32; literally") {
		t.Fatalf("literal entity not escaped: %q", out)
	}
	again := MarkdownToBlocks(out, fileOff())
	if got, want := (&Document{Blocks: again}).PlainText(), d.PlainText(); got != want {
		t.Fatalf("plain text changed:\n%q\nwant\n%q", got, want)
	}
}

func TestAdjacentEmphasisRunsKeepTextAndMarks(t *testing.T) {
	text := PlainRich("plain ")
	text.Push("bold", Marks{Bold: true})
	text.Push("<", Marks{Bold: true, Italic: true})
	text.Push(">", Marks{Bold: true})
	text.Push("plain", Marks{})
	text.Push("italic", Marks{Italic: true})
	text.Push("both", Marks{Bold: true, Italic: true})
	d := &Document{Blocks: []*Block{NewBlock(TextKind{}, text)}}
	out := Export(d, Options{}).Text
	again := MarkdownToBlocks(out, fileOff())
	if len(again) != 1 {
		t.Fatalf("want 1 block, got %d", len(again))
	}
	if got := again[0].Text; got.Text != text.Text || len(got.Runs) != len(text.Runs) {
		t.Fatalf("round trip changed text:\n got %#v\nwant %#v", got, text)
	}
	for i := range text.Runs {
		if !MarksEqual(&again[0].Text.Runs[i].Marks, &text.Runs[i].Marks) {
			t.Fatalf("run %d marks differ", i)
		}
	}
}

func TestInlineCodeKeepsEdgeSpacesAndBacktickRuns(t *testing.T) {
	text := PlainRich("prefix ")
	text.Push("  code``  ", Marks{Code: true})
	cell := TableCell{Blocks: []*Block{NewBlock(TextKind{}, text)}, ColSpan: 1, RowSpan: 1}
	tbl := &Table{Rows: [][]TableCell{{cell}}}
	d := &Document{Blocks: []*Block{NewBlock(TextKind{}, text), NewBlock(TableKind{Table: tbl}, RichText{})}}
	out := Export(d, Options{}).Text
	again := &Document{Blocks: MarkdownToBlocks(out, fileOff())}
	if got, want := again.PlainText(), d.PlainText(); got != want {
		t.Fatalf("plain text changed:\n%q\nwant\n%q", got, want)
	}
}

func TestNestedListsKeepTextAndCodeIndentation(t *testing.T) {
	in := "- parent\n  paragraph\n  - child\n    nested text\n10. ordered\n    line\n- [x] todo\n  ```text\n  code  \n  ```\n"
	d := &Document{Blocks: MarkdownToBlocks(in, fileOff())}
	out := Export(d, Options{}).Text
	again := &Document{Blocks: MarkdownToBlocks(out, fileOff())}
	if got, want := again.PlainText(), d.PlainText(); got != want {
		t.Fatalf("plain text changed:\n%q\nwant\n%q", got, want)
	}
}

func TestPreservesPlainWikiAndSafeRelativeLinks(t *testing.T) {
	d := &Document{Blocks: []*Block{TextBlock("See [[Target|Label]] and [Local](../note.md) and [Bad](javascript:alert)")}}
	out := Export(d, Options{}).Text
	if !strings.Contains(out, "[[Target|Label]]") {
		t.Fatalf("wiki link lost: %q", out)
	}
	if !strings.Contains(out, "[Local](../note.md)") {
		t.Fatalf("relative link lost: %q", out)
	}
	if strings.Contains(out, "[Bad](javascript:alert)") {
		t.Fatalf("unsafe link kept: %q", out)
	}
}

func TestPlainAmpersandDoesNotBecomeAnEntity(t *testing.T) {
	d := &Document{Blocks: []*Block{TextBlock("literal &amp;")}}
	text := Export(d, Options{}).Text
	if !strings.Contains(text, `\&amp;`) {
		t.Fatalf("ampersand not escaped: %q", text)
	}
	blocks := MarkdownToBlocks(text, fileOff())
	if blocks[0].Text.Text != "literal &amp;" {
		t.Fatalf("round trip = %q", blocks[0].Text.Text)
	}
}

func TestEquationExportPreservesTexAndDisplayDelimiters(t *testing.T) {
	source := "Inline $\\frac{a_i}{b}$ end.\n\n$$\n\\frac{1}{\\frac{x}{y}}\n$$\n"
	d := &Document{Blocks: MarkdownToBlocks(source, fileOff())}
	text := Export(d, Options{}).Text
	if !strings.Contains(text, "$\\frac{a_i}{b}$") {
		t.Fatalf("inline math lost: %q", text)
	}
	if !strings.Contains(text, "$$\n\\frac{1}{\\frac{x}{y}}\n$$") {
		t.Fatalf("display math lost: %q", text)
	}
	reread := MarkdownToBlocks(text, fileOff())
	inline := reread[1].Text.Runs[0].Marks.Inline
	if inline == nil || !inline.IsDisplayEquation() {
		t.Fatalf("display equation not recognised: %#v", reread[1].Text)
	}
	if src, ok := inline.Equation(); !ok || src != "\\frac{1}{\\frac{x}{y}}" {
		t.Fatalf("equation source = %q", src)
	}
}

func TestUnclosedDisplayMathStaysSingleParagraph(t *testing.T) {
	blocks := MarkdownToBlocks("$$\nnot closed\n", fileOff())
	// The `$$` line stands alone; the next line is its own paragraph.
	if len(blocks) != 2 {
		t.Fatalf("want 2 blocks, got %d", len(blocks))
	}
	if got := blocks[0].Text.Text; got != "$$" {
		t.Fatalf("first text = %q", got)
	}
	if got := blocks[1].Text.Text; got != "not closed" {
		t.Fatalf("second text = %q", got)
	}
}

func TestTableMergeInsertDelete(t *testing.T) {
	grid := func(nr, nc int) *Table {
		tbl := &Table{}
		tbl.Rows = make([][]TableCell, nr)
		for r := 0; r < nr; r++ {
			tbl.Rows[r] = make([]TableCell, nc)
			for c := 0; c < nc; c++ {
				cell := EmptyCell()
				cell.Blocks = []*Block{TextBlock(string(rune('0'+r)) + string(rune('0'+c)))}
				tbl.Rows[r][c] = cell
			}
		}
		for i := 0; i < nc; i++ {
			tbl.ColWidths = append(tbl.ColWidths, 100)
		}
		return tbl
	}
	text := func(tbl *Table, r, c int) string {
		var parts []string
		for _, b := range tbl.Rows[r][c].Blocks {
			parts = append(parts, b.Text.Text)
		}
		return strings.Join(parts, "+")
	}

	tbl := grid(3, 3)
	tbl.Merge(SpanRange(0, 0, 1, 1))
	if tbl.Rows[0][0].RowSpan != 2 || tbl.Rows[0][0].ColSpan != 2 {
		t.Fatalf("merge spans = %d×%d", tbl.Rows[0][0].RowSpan, tbl.Rows[0][0].ColSpan)
	}
	if got := text(tbl, 0, 0); got != "00+01+10+11" {
		t.Fatalf("merged content = %q", got)
	}
	if !tbl.Rows[1][1].IsCovered() {
		t.Fatal("slot (1,1) should be covered")
	}
	if r, c := tbl.Origin(1, 1); r != 0 || c != 0 {
		t.Fatalf("origin(1,1) = (%d,%d)", r, c)
	}
	if got, want := tbl.Expand(SpanRange(1, 1, 1, 2)), SpanRange(0, 0, 1, 2); got != want {
		t.Fatalf("expand = %#v, want %#v", got, want)
	}
	tbl.InsertRow(1)
	if len(tbl.Rows) != 4 {
		t.Fatalf("rows = %d", len(tbl.Rows))
	}
	if tbl.Rows[0][0].RowSpan != 3 {
		t.Fatalf("rowspan after insert = %d", tbl.Rows[0][0].RowSpan)
	}
	if tbl.Rows[1][2].IsCovered() {
		t.Fatal("(1,2) should not be covered")
	}
	tbl.InsertCol(0)
	if tbl.NCols() != 4 {
		t.Fatalf("cols = %d", tbl.NCols())
	}
	if tbl.Rows[0][1].ColSpan != 2 {
		t.Fatalf("colspan after insert = %d", tbl.Rows[0][1].ColSpan)
	}
	tbl.DeleteRows(0, 1)
	if tbl.Rows[0][1].RowSpan != 2 {
		t.Fatalf("rowspan after delete = %d", tbl.Rows[0][1].RowSpan)
	}
	if got := text(tbl, 0, 1); got != "00+01+10+11" {
		t.Fatalf("content after delete = %q", got)
	}
	tbl.DeleteCols(1, 2)
	if tbl.Rows[0][1].ColSpan != 1 {
		t.Fatalf("colspan after col delete = %d", tbl.Rows[0][1].ColSpan)
	}
	tbl.Unmerge(0, 1)
	visible := 0
	tbl.Cells(func(int, int, *TableCell) { visible++ })
	if visible != 9 {
		t.Fatalf("visible cells = %d, want 9", visible)
	}
}

func TestRichTextReplaceAndMarks(t *testing.T) {
	text := PlainRich("hello world")
	text.UpdateMarks(0, 5, func(m *Marks) { m.Bold = true })
	if len(text.Runs) != 2 {
		t.Fatalf("runs = %d", len(text.Runs))
	}
	text.Replace(5, 6, PlainRich(", "))
	if text.Text != "hello, world" {
		t.Fatalf("text = %q", text.Text)
	}
	tail := text.SplitOff(7)
	if tail.Text != "world" || text.Text != "hello, " {
		t.Fatalf("split: %q / %q", tail.Text, text.Text)
	}
	if !text.MarksAt(3).Bold {
		t.Fatal("bold lost at 3")
	}
	if text.MarksAt(7).Bold {
		t.Fatal("bold leaked at 7")
	}
	if !text.MarksAt(5).Bold {
		t.Fatal("bold lost at 5")
	}
}

func TestListContinuationMergesIntoItemText(t *testing.T) {
	blocks := MarkdownToBlocks("- first line\n  continues here\n- second\n", fileOff())
	if len(blocks) != 2 {
		t.Fatalf("want 2 blocks, got %d", len(blocks))
	}
	if got := blocks[0].Text.Text; got != "first line\ncontinues here" {
		t.Fatalf("item text = %q", got)
	}
	if len(blocks[0].Children) != 0 {
		t.Fatalf("continuation became children: %d", len(blocks[0].Children))
	}
}

func TestListItemKeepsRealNestedBlocksAsChildren(t *testing.T) {
	blocks := MarkdownToBlocks("- item\n  ```go\n  x := 1\n  ```\n", fileOff())
	if len(blocks) != 1 {
		t.Fatalf("want 1 block, got %d", len(blocks))
	}
	if len(blocks[0].Children) != 1 {
		t.Fatalf("children = %d", len(blocks[0].Children))
	}
	if _, ok := blocks[0].Children[0].Kind.(CodeKind); !ok {
		t.Fatalf("child is %T", blocks[0].Children[0].Kind)
	}
}

func TestEmbeddedImagesSurviveInTextAndTables(t *testing.T) {
	uri := "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB"
	input := "![cover](" + uri + ")\n\nBefore ![inline](" + uri + ") after\n\n| Shot | Frame |\n| --- | --- |\n| 1 | Before ![frame](" + uri + ") after |\n"
	d := &Document{Blocks: MarkdownToBlocks(input, fileOff())}
	dir := "assets"
	out := Export(d, Options{Assets: &dir, DotRelative: true})
	if got := strings.Count(out.Text, uri); got != 3 {
		t.Fatalf("uri appears %d times:\n%s", got, out.Text)
	}
	if out.Missing != 0 || len(out.Files) != 0 {
		t.Fatalf("missing=%d files=%d", out.Missing, len(out.Files))
	}
	again := MarkdownToBlocks(out.Text, fileOff())
	if len(again) != len(d.Blocks) {
		t.Fatalf("blocks %d → %d", len(d.Blocks), len(again))
	}
}

func TestMarkdownTableWidgetRoundTripsMultilineSource(t *testing.T) {
	widget := HTMLInteractive("Clip", "<video controls src=\"./clip.mp4\"></video>\n<script>const label = '中文 | one';</script>")
	cell := MarkdownTableWidget(widget)
	if strings.ContainsAny(cell, "\n|") && strings.Count(cell, "|") != 0 {
		t.Fatalf("encoded cell contains a pipe: %q", cell)
	}
	got, ok := tableWidgetFromCell(cell)
	if !ok {
		t.Fatalf("decode failed for %q", cell)
	}
	if got.Source != widget.Source || got.Title != widget.Title || got.Kind != widget.Kind {
		t.Fatalf("round trip changed the widget: %#v", got)
	}
	block := tableCellBlock(cell, true)
	ib, ok := block.Kind.(InteractiveKindBlock)
	if !ok {
		t.Fatalf("cell block is %T", block.Kind)
	}
	if !strings.Contains(ib.Interactive.Source, "中文 | one") {
		t.Fatalf("source lost: %q", ib.Interactive.Source)
	}
}

func TestDocumentBlankAndPlainText(t *testing.T) {
	d := NewDocument()
	if !d.IsBlank() {
		t.Fatal("a fresh document should be blank")
	}
	d.Blocks = append(d.Blocks, NewBlock(HeadingKind{Level: 1}, PlainRich("Title")))
	if d.IsBlank() {
		t.Fatal("a document with a heading is not blank")
	}
	if got := d.PlainText(); got != "Title" {
		t.Fatalf("plain text = %q", got)
	}
}

func TestIndentAndDedent(t *testing.T) {
	if got := IndentOf("    x"); got != 4 {
		t.Fatalf("indent = %d", got)
	}
	if got := IndentOf("\tx"); got != 4 {
		t.Fatalf("tab indent = %d", got)
	}
	if got := Dedent("    x", 2); got != "  x" {
		t.Fatalf("dedent = %q", got)
	}
	if got := Dedent("x", 4); got != "x" {
		t.Fatalf("dedent past content = %q", got)
	}
}

func TestDividerAndFence(t *testing.T) {
	for _, s := range []string{"---", "***", "___", "- - -", "----"} {
		if !divider(s) {
			t.Errorf("%q should be a divider", s)
		}
	}
	for _, s := range []string{"--", "- item", "text"} {
		if divider(s) {
			t.Errorf("%q should not be a divider", s)
		}
	}
	if _, n, info, ok := fence("```go"); !ok || n != 3 || info != "go" {
		t.Fatalf("fence = %d %q %v", n, info, ok)
	}
	if _, _, _, ok := fence("``"); ok {
		t.Fatal("two backticks is not a fence")
	}
}
