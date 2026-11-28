package app

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func TestNativeMathPaintsVectorGlyphs(t *testing.T) {
	fg := ui.RGB(0, 0, 0)
	for _, formula := range []string{`\frac{\sqrt{a^2+b^2}}{2}`, `\begin{pmatrix} a & b \\ c & d \end{pmatrix}`} {
		img := renderMath(mathKey{source: formula, color: fg, size: 20, display: true})
		if img.source == nil || img.width < 10 || img.ascent+img.descent < 20 {
			t.Fatalf("formula was not typeset: %+v", img)
		}
		raster := ui.Render(func(c *ui.Context) {
			ui.Box(c).Fill().Background(ui.RGB(255, 255, 255)).Children(func() {
				ui.Image(c, img.source).Width(img.width).Height(img.ascent + img.descent)
			})
		}, 280, 130, 1)
		ink := 0
		for i := 0; i < len(raster.Pix); i += 4 {
			if raster.Pix[i+3] > 0 && raster.Pix[i] < 128 {
				ink++
			}
		}
		if ink < 60 {
			t.Fatalf("formula SVG painted only %d dark pixels", ink)
		}
	}
}

func TestEquationInsertionFromTypingAndEditShortcut(t *testing.T) {
	isolateConfig(t)
	a := New("")
	id := a.core.Doc.Blocks[0].ID
	a.focusAt(editor.Caret(editor.Pos{Block: id}))
	tr := ui.NewTester(a.View, 920, 640)
	tr.Frame()
	tr.Type("$$")
	tr.Frame()
	if a.equation.open {
		t.Fatal("typing a delimiter activated the formula before Enter")
	}
	tr.Key(0, ui.KeyEnter)
	tr.Frame()
	if _, ok := tr.Find("LaTeX 源码"); !ok {
		t.Fatal("Enter did not expose the formula input")
	}
	const src = `\frac{1}{x_1}`
	tr.Type(src)
	tr.Key(ui.Cmd, ui.KeyEnter)
	tr.Frame()
	b := a.core.Doc.Blocks[0]
	if b.ID != id || a.equation.open || !strings.Contains(a.markdownSource(), "$$\n"+src+"\n$$") {
		t.Fatalf("formula insert left %q", a.markdownSource())
	}
	if a.inlineAttachment(b.Text.Runs[0].Marks.Inline).Image == nil {
		t.Fatal("display equation is missing its native image")
	}
	a.focusAt(editor.Caret(editor.Pos{Block: id}))
	tr.Frame()
	mods, key := ui.Cmd|ui.Shift, ui.KeyM
	if ui.Cmd == ui.Super {
		mods, key = ui.Cmd|ui.Alt, ui.KeyB
	}
	tr.Key(mods, key)
	tr.Frame()
	if a.equation.source != src || a.equation.replace != id || !a.equation.open {
		t.Fatal("math shortcut did not reopen the existing equation")
	}
}

func TestLaTeXDelimitersSurviveWorkspaceRead(t *testing.T) {
	const raw = "Use \\(x_i\\) here.\n\n\\[\n\\sqrt{x}\n\\]\n"
	d := doc.ReadSource(raw)
	if got, _ := d.WorkspaceMarkdown(); got != raw {
		t.Fatalf("LaTeX delimiters changed to %q", got)
	}
	if len(d.Blocks) != 2 {
		t.Fatalf("expected inline and display formula blocks, got %d", len(d.Blocks))
	}
	if got, ok := displayEquation(d.Blocks[1]); !ok || got != `\sqrt{x}` {
		t.Fatalf("display math = %q", got)
	}
	for _, run := range d.Blocks[0].Text.Runs {
		if src, ok := run.Marks.Inline.Equation(); ok && src == "x_i" {
			return
		}
	}
	t.Fatal("inline math remained plain text")
}
