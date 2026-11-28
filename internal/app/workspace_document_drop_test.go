package app

import (
	"bytes"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/neko233/markdownx/internal/doc"
)

const workspaceDropImageName = "封面 [修订] (红).png"
const workspaceDropImagePath = "../assets/" + workspaceDropImageName

func workspaceDocumentDropFixture(t *testing.T, text string) (*App, *ui.Tester, string, string) {
	t.Helper()
	isolateConfig(t)
	root := t.TempDir()
	for _, dir := range []string{"assets", "docs"} {
		if err := os.Mkdir(filepath.Join(root, dir), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	current := filepath.Join(root, "docs", "Main.md")
	if err := os.WriteFile(current, []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
	asset := filepath.Join(root, "assets", workspaceDropImageName)
	cover := image.NewRGBA(image.Rect(0, 0, 84, 46))
	draw.Draw(cover, cover.Bounds(), image.NewUniform(color.RGBA{R: 171, G: 57, B: 64, A: 255}), image.Point{}, draw.Src)
	var data bytes.Buffer
	if err := png.Encode(&data, cover); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(asset, data.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}
	saveSettings(settings{TreeFilter: workspaceFilter{Mode: workspaceAllFiles}})
	a := New(root)
	a.OpenFile(current)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	workspaceTestClick(t, tr, "assets")
	return a, tr, current, asset
}

func dragWorkspaceDocumentEntry(t *testing.T, tr *ui.Tester, label string, x, y float32) {
	t.Helper()
	row, ok := workspaceTreeRow(tr, label)
	if !ok {
		t.Fatalf("cannot find tree entry %q", label)
	}
	tr.Press(row.X+row.W/2, row.Y+row.H/2)
	tr.Move(x, y)
	tr.Release(x, y)
}

func workspaceSourceCaretPoint(t *testing.T, tr *ui.Tester, s *sourceView, at int) (float32, float32) {
	t.Helper()
	r, ok := tr.Find("Markdown 源码")
	if !ok {
		t.Fatal("cannot find the source editor")
	}
	caret, ok := s.edit.Rect(at)
	if !ok || caret.Y < 0 || caret.Y+caret.H > r.H {
		t.Fatalf("source caret at %d is outside the viewport: %+v", at, caret)
	}
	return r.X + caret.X + 0.1, r.Y + caret.Y + caret.H/2
}

func assertWorkspaceImageDestination(t *testing.T, source string) {
	t.Helper()
	targets := doc.ImageDestinations(source)
	if len(targets) != 1 || !strings.Contains(targets[0].Path, "%20") || !strings.Contains(targets[0].Path, "%28") {
		t.Fatalf("image reference does not preserve URL escapes: %q", source)
	}
	if path, err := url.PathUnescape(targets[0].Path); err != nil || path != workspaceDropImagePath {
		t.Fatalf("image path = %q, want %q (%v)", path, workspaceDropImagePath, err)
	}
}

func TestFileTreeDragImageInVisualEditor(t *testing.T) {
	const original = "封面见这里\n"
	a, tr, current, asset := workspaceDocumentDropFixture(t, original)
	p := a.bufs[a.core.Doc.Blocks[0].ID]
	caret := p.layout(0, p.bounds.W).Caret(utf16Index(p.visible, len("封面")))
	before, err := os.ReadFile(asset)
	if err != nil {
		t.Fatal(err)
	}
	dragWorkspaceDocumentEntry(t, tr, workspaceDropImageName, p.bounds.X+caret.X+0.1, p.bounds.Y+caret.Y+caret.H/2)
	b := a.core.Doc.Blocks[0]
	if !a.dirty || b.Text.Text != "封面"+doc.Obj+"见这里" || len(b.Text.Runs) != 3 {
		t.Fatalf("image did not land at the pointer: %+v", b.Text)
	}
	img, ok := b.Text.Runs[1].Marks.Inline.Image()
	if !ok || img.Alt != workspaceDropImageName {
		t.Fatalf("dropped inline image = %+v", img)
	}
	if loaded, err := a.md.LoadImage(img.Src); err != nil || loaded == nil {
		t.Fatalf("embedded PNG did not load: %v", err)
	}
	want := a.markdownSource()
	assertWorkspaceImageDestination(t, want)
	tr.Key(ui.Cmd, ui.KeyZ)
	if a.dirty || a.markdownSource() != original {
		t.Fatalf("one undo did not restore the original document: %q", a.markdownSource())
	}
	tr.Key(ui.Cmd|ui.Shift, ui.KeyZ)
	tr.Key(ui.Cmd, ui.KeyS)
	if saved, err := os.ReadFile(current); err != nil || string(saved) != want {
		t.Fatalf("redo and save = %q, want %q (%v)", saved, want, err)
	}
	if after, err := os.ReadFile(asset); err != nil || !bytes.Equal(after, before) {
		t.Fatalf("drag changed the source asset: %v", err)
	}
}

func TestFileTreeDragImageInBlankParagraph(t *testing.T) {
	a, tr, _, _ := workspaceDocumentDropFixture(t, "")
	r := a.bufs[a.core.Doc.Blocks[0].ID].bounds
	dragWorkspaceDocumentEntry(t, tr, workspaceDropImageName, r.X+5, r.Y+r.H/2)
	blocks := a.core.Doc.Blocks
	if _, ok := blocks[0].Kind.(doc.ImageKind); !ok || len(blocks) != 2 || !blocks[1].Text.IsEmpty() || a.focus != blocks[1].ID {
		t.Fatalf("image drop did not leave an editable paragraph: %q", a.markdownSource())
	}
	assertWorkspaceImageDestination(t, a.markdownSource())
	saveWorkspaceSnapshot(t, tr, "27-workspace-dropped-image")
	tr.Key(ui.Cmd, ui.KeyZ)
	if a.dirty || a.markdownSource() != "" {
		t.Fatalf("one undo did not restore the empty document: %q", a.markdownSource())
	}
}

func TestFileManagerDropInVisualDocumentUsesTheSameReferences(t *testing.T) {
	const original = "开头结尾\n"
	a, tr, current, image := workspaceDocumentDropFixture(t, original)
	external := t.TempDir()
	note := filepath.Join(external, "外部 [资料] #1.md")
	if err := os.WriteFile(note, []byte("# 外部\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	p := a.bufs[a.core.Doc.Blocks[0].ID]
	caret := p.layout(0, p.bounds.W).Caret(utf16Index(p.visible, len("开头")))
	x, y := p.bounds.X+caret.X+0.1, p.bounds.Y+caret.Y+caret.H/2
	tr.Move(x, y)
	if !tr.DropFiles(x, y, image, note, external) {
		t.Fatal("the visual document declined the native file-list target")
	}
	got := a.markdownSource()
	if !a.dirty || !strings.HasPrefix(got, "开头![") || !strings.HasSuffix(got, "结尾\n") {
		t.Fatalf("the file list did not land at the caret: %q", got)
	}
	assertWorkspaceImageDestination(t, got)
	var foundNote, foundFolder bool
	for _, run := range a.core.Doc.Blocks[0].Text.Runs {
		if run.Marks.Link != nil {
			path, info := a.linkedDocument(*run.Marks.Link)
			foundNote = foundNote || info != nil && info.Mode().IsRegular() && samePath(path, note)
			foundFolder = foundFolder || info != nil && info.IsDir() && samePath(path, external)
		}
	}
	if !foundNote || !foundFolder {
		t.Fatalf("the dropped file and folder did not resolve: %q", got)
	}
	if files, err := os.ReadDir(filepath.Dir(current)); err != nil || len(files) != 1 {
		t.Fatalf("inserting references created additional files: %v (%v)", files, err)
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	if a.dirty || a.markdownSource() != original {
		t.Fatalf("undo left part of the batch behind: %q", a.markdownSource())
	}
}

func TestFileManagerDropOnDocumentWhitespaceAppendsAfterCode(t *testing.T) {
	const original = "```text\n原代码\n```\n"
	a, tr, _, image := workspaceDocumentDropFixture(t, original)
	x, y := a.editBounds.X+a.editBounds.W/2, a.editBounds.Y+a.editBounds.H-24
	tr.Move(x, y)
	if !tr.DropFiles(x, y, image) {
		t.Fatal("the whitespace below the document did not accept the drop")
	}
	blocks := a.core.Doc.Blocks
	if len(blocks) != 3 || blocks[0].Text.Text != "原代码" || !blocks[2].Text.IsEmpty() || a.focus != blocks[2].ID {
		t.Fatalf("the drop changed the code or did not leave an editable paragraph: %q", a.markdownSource())
	}
	if _, ok := blocks[1].Kind.(doc.ImageKind); !ok {
		t.Fatalf("the drop did not create a standalone image: %q", a.markdownSource())
	}
	assertWorkspaceImageDestination(t, a.markdownSource())
	tr.Key(ui.Cmd, ui.KeyZ)
	if a.dirty || a.markdownSource() != original {
		t.Fatalf("undo did not restore the original code: %q", a.markdownSource())
	}
}

func TestFileManagerDropInSourceUsesLiteralPathsInCode(t *testing.T) {
	const before, after = "开头\n\n```text\n位置", "\n```\n\n结尾\n"
	a, tr, current, image := workspaceDocumentDropFixture(t, before+after)
	note := filepath.Join(filepath.Dir(current), "资料 (二).md")
	if err := os.WriteFile(note, []byte("# 资料\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a.toggleSource()
	tr.Frame()
	x, y := workspaceSourceCaretPoint(t, tr, a.source, len(before))
	a.source.edit.Select(0, len("开头"))
	tr.Move(x, y)
	if !tr.DropFiles(x, y, image, note) {
		t.Fatal("the Markdown source did not accept the file list")
	}
	if want := before + workspaceDropImagePath + "\n资料 (二).md" + after; a.source.text() != want {
		t.Fatalf("the drop did not preserve the literal paths and insertion location: %q", a.source.text())
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	if a.dirty || a.source.text() != before+after {
		t.Fatalf("undo did not restore the source: %q", a.source.text())
	}
}

func TestUntitledDropReferencesStillResolveAfterSavingElsewhere(t *testing.T) {
	isolateConfig(t)
	note := filepath.Join(t.TempDir(), "另外的文件 #1.md")
	if err := os.WriteFile(note, []byte("# 资料\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New("")
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	p := a.bufs[a.core.Doc.Blocks[0].ID]
	x, y := p.bounds.X+4, p.bounds.Y+p.bounds.H/2
	tr.Move(x, y)
	if !tr.DropFiles(x, y, note) {
		t.Fatal("the untitled document did not accept the drop")
	}
	saved := filepath.Join(t.TempDir(), "Saved.md")
	a.saveMarkdown(saved)
	reopened := New(saved)
	runs := reopened.core.Doc.Blocks[0].Text.Runs
	if len(runs) != 1 || runs[0].Marks.Link == nil || !strings.HasPrefix(*runs[0].Marks.Link, "file:///") {
		t.Fatalf("the absolute link did not survive save and reload: %q", reopened.markdownSource())
	}
	if path, info := reopened.linkedDocument(*runs[0].Marks.Link); info == nil || !samePath(path, note) {
		t.Fatalf("the link resolved relative to the save folder: %q", path)
	}
}

func TestTreeDragIntoAnotherWorkspaceReferencesTheOriginalFile(t *testing.T) {
	isolateConfig(t)
	other := t.TempDir()
	origin := filepath.Join(other, "不同工作区.md")
	if err := os.WriteFile(origin, []byte("# 来源\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	workspace := New(other)
	receiverRoot := t.TempDir()
	current := filepath.Join(receiverRoot, "当前.md")
	if err := os.WriteFile(current, []byte("前后\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	receiver := New(current)
	tr := ui.NewTester(func(c *ui.Context) {
		ui.Row(c).Fill().Children(func() {
			row := ui.Button(c, "不同工作区.md").Width(145).Role(ui.RoleTreeItem)
			workspace.dragTreeEntry(row, workspace.tree.at(origin))
			ui.Column(c).Grow(1).FillHeight().Children(func() { receiver.View(c) })
		})
	}, 1080, 680)
	tr.Frame()
	p := receiver.bufs[receiver.core.Doc.Blocks[0].ID]
	caret := p.layout(0, p.bounds.W).Caret(utf16Index(p.visible, len("前")))
	dragWorkspaceDocumentEntry(t, tr, "不同工作区.md", p.bounds.X+caret.X+0.1, p.bounds.Y+caret.Y+caret.H/2)
	b := receiver.core.Doc.Blocks[0]
	if len(b.Text.Runs) != 3 || b.Text.Runs[1].Marks.Link == nil {
		t.Fatalf("the destination rejected a tree entry from the other workspace: %q", receiver.markdownSource())
	}
	if dest, info := receiver.linkedDocument(*b.Text.Runs[1].Marks.Link); info == nil || !samePath(dest, origin) {
		t.Fatalf("the cross-workspace reference points to %q", dest)
	}
}

func TestFileTreeDragInMarkdownSource(t *testing.T) {
	for _, tc := range []struct{ name, before, after string }{
		{"image", "正文 ", "\n"},
		{"code block", "```text\n  位置", "\n```\n"},
		{"inline code", "前缀 ``字面", "值`` 后缀\n"},
		{"inline math", "前缀 $a", "+b$ 后缀\n"},
		{"display math", "$$\na", "+b\n$$\n"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			original := tc.before + tc.after
			a, tr, _, _ := workspaceDocumentDropFixture(t, original)
			a.toggleSource()
			tr.Frame()
			_, firstRune := utf8.DecodeRuneInString(original)
			a.source.edit.Select(0, firstRune)
			x, y := workspaceSourceCaretPoint(t, tr, a.source, len(tc.before))
			dragWorkspaceDocumentEntry(t, tr, workspaceDropImageName, x, y)
			got := a.source.text()
			if tc.name == "image" {
				assertWorkspaceImageDestination(t, got)
				if !strings.HasPrefix(got, tc.before+"![") || !strings.HasSuffix(got, tc.after) {
					t.Fatalf("image did not land at the pointer: %q", got)
				}
			} else if want := tc.before + workspaceDropImagePath + tc.after; got != want {
				t.Fatalf("literal source drop = %q, want %q", got, want)
			}
			tr.Key(ui.Cmd, ui.KeyZ)
			if a.dirty || a.source.text() != original {
				t.Fatalf("one undo did not restore the original source: %q", a.source.text())
			}
		})
	}
}

func TestFileTreeSourceDropUsesWrappedScrolledPosition(t *testing.T) {
	prefix := strings.Repeat("以前的段落\n\n", 60)
	const cluster = "中文🙂e\u0301 "
	before := prefix + "句首\t" + strings.Repeat(cluster, 55) + "这里"
	after := strings.Repeat(cluster, 40) + "\n\n结尾\n"
	a, tr, _, _ := workspaceDocumentDropFixture(t, before+after)
	a.toggleSource()
	s := a.source
	tr.Frame()
	at := len(before)
	s.edit.Select(at, at)
	tr.Frame()
	first, _ := s.edit.Rect(len(prefix))
	target, _ := s.edit.Rect(at)
	if s.edit.ScrollY() <= 0 || first.Y >= target.Y {
		t.Fatal("test requires a scrolled continuation of a wrapped line")
	}
	x, y := workspaceSourceCaretPoint(t, tr, s, at)
	tr.ClickAt(x, y)
	if s.edit.Cursor() != at {
		t.Fatalf("test point selected byte %d, want %d", s.edit.Cursor(), at)
	}
	s.edit.Select(at-len("这里"), at-len("这里"))
	dragWorkspaceDocumentEntry(t, tr, workspaceDropImageName, x, y)
	got := s.text()
	if !strings.HasPrefix(got, before+"![") || !strings.HasSuffix(got, after) {
		t.Fatalf("drag did not use the scrolled and wrapped source position at %d, markup starts at %d", at, strings.Index(got, "!["))
	}
	assertWorkspaceImageDestination(t, got)
	tr.Key(ui.Cmd, ui.KeyZ)
	if s.text() != before+after || a.dirty {
		t.Fatal("undo did not restore the wrapped document")
	}
}

func TestFileTreeDragLiteralInVisualCodeAndMath(t *testing.T) {
	for _, tc := range []struct {
		name, text, before string
		revealAt           int
	}{
		{"code block", "```text\n位置\n```\n", "位", 0},
		{"inline code", "前缀 `字面值` 后缀\n", "前缀 `字面", len("前缀 字")},
		{"inline math", "前缀 $a+b$ 后缀\n", "前缀 $a", len("前缀 ")},
	} {
		t.Run(tc.name, func(t *testing.T) {
			a, tr, _, _ := workspaceDocumentDropFixture(t, tc.text)
			b := a.core.Doc.Blocks[0]
			p := a.bufs[b.ID]
			if tc.revealAt > 0 {
				clickInBlock(t, tr, p, tc.revealAt, 0)
				if a.inlineFor(b) == nil {
					t.Fatal("click did not reveal the inline source")
				}
				tr.Frame()
			}
			if !strings.HasPrefix(p.visible, tc.before) {
				t.Fatalf("editor text = %q, want prefix %q", p.visible, tc.before)
			}
			caret := p.layout(0, p.bounds.W).Caret(utf16Index(p.visible, len(tc.before)))
			dragWorkspaceDocumentEntry(t, tr, workspaceDropImageName, p.bounds.X+caret.X+0.1, p.bounds.Y+caret.Y+caret.H/2)
			want := strings.Replace(tc.text, tc.before, tc.before+workspaceDropImagePath, 1)
			if got := a.markdownSource(); !a.dirty || got != want {
				t.Fatalf("literal visual drop = %q, want %q", got, want)
			}
			tr.Key(ui.Cmd, ui.KeyZ)
			if a.dirty || a.markdownSource() != tc.text {
				t.Fatalf("one undo did not restore the original text: %q", a.markdownSource())
			}
		})
	}
}
