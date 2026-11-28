package doc

import "testing"

func TestLocalFileURLsRoundTripThroughMarkdownLinks(t *testing.T) {
	const target = "file:///D:/%E8%B5%84%E6%96%99/notes%20%231.md"
	if path, ok := LocalFileURLPath(target); !ok || path != "/D:/资料/notes #1.md" {
		t.Fatalf("decoded local path = %q (%v)", path, ok)
	}
	text := ParseInlineSource("[打开](<" + target + ">)")
	if len(text.Runs) != 1 || text.Runs[0].Marks.Link == nil || *text.Runs[0].Marks.Link != target {
		t.Fatalf("file URL did not parse as a link: %+v", text)
	}
	if got := MarkdownInline(text); got != "[打开]("+target+")" && got != "[打开](<"+target+">)" {
		t.Fatalf("file link did not survive Markdown export: %q", got)
	}
	for _, unsafe := range []string{"file://host/share/document.md", "file:relative.md", "file:////host/share", "file:///%2fhost/share", "file:///D:/bad%00name", "file:///D:/bad%5Cname"} {
		if _, ok := LocalFileURLPath(unsafe); ok {
			t.Fatalf("accepted a nonlocal or invalid file path: %q", unsafe)
		}
	}
}
