package app

import (
	"bytes"
	"image/jpeg"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	syntax "github.com/egoist/mygo/ui/editor"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func TestDiagramPreferenceSourceEditingAndCopy(t *testing.T) {
	dir := isolateConfig(t)
	path := filepath.Join(dir, "diagram.md")
	const diagramSource = "flowchart LR\n  A[准备] --> B[检查]"
	const text = "# 实施计划\n\n~~~~ MeRmAiD {#plan}  \n" + diagramSource + "\n~~~~~\n"
	const clickLabel = "编辑图表源码"
	if err := os.WriteFile(path, []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	tr := ui.NewTester(a.View, 980, 720)
	tr.Frame()
	if _, ok := tr.Access(clickLabel); ok {
		t.Fatal("diagrams previewed before enabling the setting")
	}
	a.prefsOpen = true
	tr.Frame()
	if err := tr.Click("渲染 Markdown 图表"); err != nil {
		t.Fatal(err)
	}
	if err := tr.Click("完成"); err != nil {
		t.Fatal(err)
	}
	if !loadSettings().Diagrams {
		t.Fatal("diagram preference did not persist")
	}
	if err := tr.RightClick(clickLabel); err != nil {
		t.Fatal(err)
	}
	if err := tr.ChooseMenuItem("复制图表"); err != nil {
		t.Fatal(err)
	}
	clip := tr.ClipboardData()
	svg, err := clip.Read(diagramSVG)
	if err != nil || !bytes.Contains(svg, []byte("检查")) {
		t.Fatalf("missing SVG on clipboard: %v", err)
	}
	data, err := clip.Read(transfer.PNG)
	if err != nil {
		t.Fatal(err)
	}
	m, err := png.Decode(bytes.NewReader(data))
	if err != nil || m.Bounds().Dx() < 30 || m.Bounds().Dy() < 20 {
		t.Fatalf("invalid diagram PNG: %v", err)
	}
	if err := tr.Click(clickLabel); err != nil {
		t.Fatal(err)
	}
	if !tr.Focused("图表源码") {
		t.Fatal("diagram click did not focus source")
	}
	b := a.core.Doc.Blocks[1]
	a.focusAt(editor.Caret(editor.Pos{Block: b.ID, Offset: b.Text.Len()}))
	tr.Frame()
	tr.Type("\n  B --> C[完成]")
	tr.Key(0, ui.KeyEscape)
	if _, ok := tr.Access("图表源码"); ok {
		t.Fatal("Escape did not return to the rendered diagram")
	}
	if !tr.Focused(clickLabel) {
		t.Fatal("focus was lost when closing the source")
	}
	want := strings.Replace(text, "B[检查]", "B[检查]\n  B --> C[完成]", 1)
	a.Save()
	if actual, err := os.ReadFile(path); err != nil || string(actual) != want {
		t.Fatalf("saved diagram changed Markdown: %q (%v)", actual, err)
	}
	a.mode = ModePreview
	tr.Frame()
	if _, ok := tr.AccessByRole(ui.RoleImage, "Markdown 图表"); !ok {
		t.Fatal("preview view did not render the edited diagram")
	}
}

func TestDiagramErrorKeepsEditableSourceAndJPEGRaster(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("```mermaid\nflowchart LR\n  A[unterminated\n```\n\n```mermaid\nflowchart LR\n  B --> C\n```\n")
	a.diagrams = true
	tr := ui.NewTester(a.View, 900, 680)
	tr.Frame()
	if _, ok := tr.Access("图表源码"); !ok {
		t.Fatal("invalid diagram lost its source field")
	}
	if _, ok := tr.Access("编辑图表源码"); !ok {
		t.Fatal("invalid diagram affected the following diagram")
	}
	light := ui.LightTheme()
	img := a.diagramImage("mermaid", "flowchart LR\n  B --> C", light)
	if img.vector == nil {
		t.Fatal(img.err)
	}
	data, err := img.encode("jpg", light.Background)
	if err != nil {
		t.Fatal(err)
	}
	if decoded, err := jpeg.Decode(bytes.NewReader(data)); err != nil || decoded.Bounds().Empty() {
		t.Fatalf("invalid JPEG export: %v", err)
	}
}

func TestCodeFenceLanguageHighlightAndSelectAll(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("旁边的正文\n\n```go\npackage main\n/* multi\n  line comment */\nvar x = 42\n```\n\n结尾\n")
	b := a.core.Doc.Blocks[1]
	a.focusAt(editor.Caret(editor.Pos{Block: b.ID}))
	tr := ui.NewTester(a.View, 920, 680)
	tr.Frame()
	p := a.bufs[b.ID]
	keyword := syntax.DefaultTheme(ui.LightTheme()).Tokens[syntax.TokenKeyword].Color
	if len(p.layouts[0].spans) < 2 || p.layouts[0].spans[0].Text != "package" || p.layouts[0].spans[0].Color != keyword {
		t.Fatalf("code syntax was not applied to the native text layout: %+v", p.layouts[0].spans)
	}
	comment := syntax.DefaultTheme(ui.LightTheme()).Tokens[syntax.TokenComment].Color
	if spans := p.layouts[2].spans; len(spans) == 0 || spans[0].Color != comment {
		t.Fatal("multiline tokenizer state was lost")
	}
	tr.Key(ui.Cmd, ui.KeyA)
	if a.core.Sel.AcrossBlocks() || a.core.SelectedText() != b.Text.Text {
		t.Fatal("select all inside code included surrounding prose")
	}
	tr.Key(ui.Cmd, ui.KeyC)
	if copied := tr.Clipboard(); copied != b.Text.Text {
		t.Fatalf("copied code = %q", copied)
	}
	if err := tr.Click("代码块语言"); err != nil {
		t.Fatal(err)
	}
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Type("python")
	tr.Key(0, ui.KeyEnter)
	if lang := b.Kind.(doc.CodeKind).Lang; lang != "python" || !strings.Contains(a.markdownSource(), "```python\npackage main") {
		t.Fatalf("language change did not reach the fence: %q", lang)
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	if a.core.Block(b.ID).Kind.(doc.CodeKind).Lang != "go" || a.core.Block(b.ID).Text.Text != b.Text.Text {
		t.Fatal("undo did not restore the previous fence language")
	}
}
