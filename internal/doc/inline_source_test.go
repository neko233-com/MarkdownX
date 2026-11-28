package doc

import (
	"strings"
	"testing"
)

func TestPreserveInlineSourcesAfterAChangeElsewhere(t *testing.T) {
	const markup = `前缀 __粗体__ 和 [路径](<notes/稿件 (一).md#标题> "保留标题 (一)")。`
	previous := ParseInlineSource(markup)
	// Native attributes keep the parsed styles, without the Markdown spelling.
	parsed := mdInlineWith(markup, true, true)
	var next RichText
	for _, span := range parsed.Spans() {
		next.Push(parsed.Text[span.Start:span.End], span.Marks)
	}
	next.Replace(0, len("前缀"), PlainRich("更换前缀"))
	next.PreserveInlineSources(mdInlineWith(markup, true, true))
	want := strings.Replace(markup, "前缀", "更换前缀", 1)
	if got := MarkdownInline(next); got != want {
		t.Fatalf("native edit wrote %q, want %q", got, want)
	}
	if previous.Text != "前缀 粗体 和 路径。" {
		t.Fatalf("parsed link title leaked into the text: %q", previous.Text)
	}
}

func TestAdjacentStyledSpansStillRoundTrip(t *testing.T) {
	text := mdInlineWith(`**重** *斜* [说明](./article "保留")`, true, true)
	text.Replace(len("重"), len("重 "), RichText{})
	markup := MarkdownInline(text)
	if !strings.Contains(markup, `[说明](./article "保留")`) || !mdInlineWith(markup, true, true).sameInlineContent(text) {
		t.Fatalf("joined inline Markdown lost marks or an unrelated link title: %q", markup)
	}
}
