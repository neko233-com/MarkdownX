package doc

import (
	"strings"
	"testing"
)

func TestReferenceRoundTripPreservesMarkersDefinitionsAndLiteralCode(t *testing.T) {
	const source = "[TOC]\n\n# 引言\n\n[阅读][Source] 和 [Source][]，先写[^二]再写[^一]。\n" +
		"\n[Source]:\n  <notes (new).md#记录>\n  \"书名 & 备注\"\n" +
		"\n[^一]: 第一条。\n\n    继续 *强调*。\n\n[^二]: 第二条。\n" +
		"\n```md\n[toc]\n[Source]: literal\n```\n"
	d := ReadSource(source)
	if _, ok := d.Blocks[0].Kind.(TOCKind); !ok {
		t.Fatal("TOC did not parse as a document block")
	}
	if _, ok := d.Blocks[len(d.Blocks)-1].Kind.(CodeKind); !ok {
		t.Fatal("the literal fenced content did not parse as code")
	}
	r := d.References()
	link, ok := r.Link(" source ")
	if !ok || link.Destination != "notes (new).md#记录" || link.Title != "书名 & 备注" {
		t.Fatalf("resolved reference = %+v", link)
	}
	ordered := r.Footnotes()
	if len(ordered) != 2 || ordered[0].Label != "二" || ordered[1].Label != "一" || len(ordered[1].Body) != 2 {
		t.Fatalf("footnote order or continuation was lost: %v", len(ordered))
	}
	if got, _ := d.WorkspaceMarkdown(); got != source {
		t.Fatalf("unchanged source = %q", got)
	}
	d.Blocks[2].Text.InsertPlain(0, "前缀 ", Marks{})
	got, _ := d.WorkspaceMarkdown()
	want := strings.Replace(source, "[阅读][Source]", "前缀 [阅读][Source]", 1)
	if got != want {
		t.Fatalf("edited source = %q, want %q", got, want)
	}
	parsed := ReadSource(got)
	if got := parsed.Blocks[2].Text.PlainText(); got != "前缀 阅读 和 Source，先写[^二]再写[^一]。" {
		t.Fatalf("recognized markers = %q", got)
	}
}
