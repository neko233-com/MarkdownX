package doc

import (
	"slices"
	"testing"
)

func TestImageDestinationsSelectAuthoredTargetsOnly(t *testing.T) {
	source := "---\ntitle: \"![metadata](unseen.svg)\"\n---\n" +
		"![a\\]b]( <./asset/a b.svg> 'caption' )\n\n" +
		"Inline ![drawing](img/p\\(1\\).png \"title\").\n\n" +
		"> Image ![quoted](img/q.svg)\n\n" +
		"![photo][Hero] ![Hero][] ![Hero]\n\n[hero]:\n  <img/hero.svg> \"reference title\"\n\n" +
		"[hero]: ignored.svg\n\n[link]: ./doc.md\n\n" +
		"`![code](unseen.svg)` and $![math](unseen.svg)$\n\n" +
		"```md\n[hero]: example.svg\n![sample](example.svg)\n```\n"
	var raw, resolved []string
	for _, dest := range ImageDestinations(source) {
		raw = append(raw, source[dest.Start:dest.End])
		resolved = append(resolved, dest.Path)
	}
	if !slices.Equal(raw, []string{"./asset/a b.svg", `img/p\(1\).png`, "img/q.svg", "img/hero.svg"}) {
		t.Fatalf("unexpected source destinations: %q", raw)
	}
	if !slices.Equal(resolved, []string{"./asset/a b.svg", "img/p(1).png", "img/q.svg", "img/hero.svg"}) {
		t.Fatalf("unexpected decoded destinations: %q", resolved)
	}
}
