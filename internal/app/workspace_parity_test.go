package app

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestFileTreeDropsOnFilesOrBlankSpaceAndExpandsHoveredFolders(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	archive := filepath.Join(root, "Archive")
	if err := os.Mkdir(archive, 0o700); err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(root, "Page.md")
	indicator := filepath.Join(archive, "Inside.md")
	for _, path := range []string{source, indicator} {
		if err := os.WriteFile(path, []byte("source"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(source)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	center := func(label string) (float32, float32) {
		t.Helper()
		box, ok := workspaceTreeRow(tr, label)
		if !ok {
			t.Fatalf("missing row %q", label)
		}
		return box.X + box.W/2, box.Y + box.H/2
	}
	x, y := center("Page.md")
	dx, dy := center("Archive")
	tr.Press(x, y)
	tr.Move(dx, dy)
	if tr.HasText("Inside.md") {
		t.Fatal("drag expanded the folder before hovering")
	}
	tr.Advance(700 * time.Millisecond)
	dx, dy = center("Inside.md")
	tr.Move(dx, dy)
	tr.Release(dx, dy)
	moved := filepath.Join(archive, "Page.md")
	if a.treeEdit.open || a.path != moved {
		t.Fatalf("file row drop did not move directly to %q: %q", moved, a.path)
	}
	x, y = center("Page.md")
	tr.Press(x, y)
	tr.Move(80, 420)
	tr.Release(80, 420)
	if a.treeEdit.open || a.path != source {
		t.Fatal("blank space did not route drops to the workspace root")
	}
	x, y = center("Page.md")
	dx, dy = center("Archive")
	tr.Press(x, y)
	tr.Move(dx, dy)
	tr.Key(ui.Alt, ui.KeyUnknown)
	if a.treeDropTarget != archive || !a.treeDropCopy {
		t.Fatal("the highlighted target did not switch to the copy operation")
	}
	tr.Key(0, ui.KeyUnknown)
	if a.treeDropCopy {
		t.Fatal("releasing Alt while hovering kept the copy operation")
	}
	tr.Key(ui.Alt, ui.KeyUnknown)
	saveWorkspaceSnapshot(t, tr, "28-workspace-copy-drop-target")
	tr.Release(dx, dy)
	if a.treeEdit.open || a.path != source || len(a.treeUndo.copies) != 1 {
		t.Fatal("Alt copy moved the live document or did not record undo")
	}
	for _, path := range []string{source, moved} {
		if data, err := os.ReadFile(path); err != nil || string(data) != "source" {
			t.Fatalf("Alt copy lost the original or the destination: %q, %v", data, err)
		}
	}
	if err := os.WriteFile(moved, []byte("changed after copy"), 0o600); err != nil {
		t.Fatal(err)
	}
	workspaceTestMenu(t, tr, "Archive", "撤销上次文件操作")
	if data, err := os.ReadFile(moved); err != nil || string(data) != "changed after copy" || !a.treeUndo.available() {
		t.Fatalf("tree undo failed to protect edited copy: %q (%v)", data, err)
	}
}

func TestFileTreeDirectDropPreservesConflictingFiles(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	dir := filepath.Join(root, "Archive")
	if err := os.Mkdir(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	from, to := filepath.Join(root, "Page.md"), filepath.Join(dir, "Page.md")
	for path, contents := range map[string]string{from: "source", to: "occupied"} {
		if err := os.WriteFile(path, []byte(contents), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(from)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	r, ok := tr.Find("Archive")
	if !ok {
		t.Fatal("missing destination folder")
	}
	dragWorkspaceDocumentEntry(t, tr, "Page.md", r.X+r.W/2, r.Y+r.H/2)
	if a.treeEdit.open || a.path != from || a.treeUndo.available() || !strings.HasPrefix(a.status, "文件操作失败") {
		t.Fatalf("colliding drop did not report failure: %q", a.status)
	}
	for path, contents := range map[string]string{from: "source", to: "occupied"} {
		if data, err := os.ReadFile(path); err != nil || string(data) != contents {
			t.Fatalf("colliding drop altered %q: %q (%v)", path, data, err)
		}
	}
	if err := os.Rename(to, filepath.Join(dir, "Existing.md")); err != nil {
		t.Fatal(err)
	}
	dragWorkspaceDocumentEntry(t, tr, "Page.md", r.X+r.W/2, r.Y+r.H/2)
	if got, err := os.ReadFile(to); err != nil || string(got) != "source" || a.path != to || !a.treeUndo.available() {
		t.Fatalf("drop after resolving collision failed: %q, %q, %v", got, a.status, err)
	}
}

func TestFileTreeOpensWorkspaceFromEmptySidebarDrop(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	doc := filepath.Join(root, "Page.md")
	if err := os.WriteFile(doc, []byte("workspace"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New("")
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	if !tr.DropFiles(80, 240, doc) || a.root != root || !workspaceTestFocused(a, tr, "Page.md") {
		t.Fatalf("empty sidebar did not open the file's workspace: %q", a.root)
	}
	workspaceTestClick(t, tr, "文件夹操作")
	if err := tr.ChooseMenuItem("关闭文件夹"); err != nil {
		t.Fatal(err)
	}
	if a.root != "" || !tr.HasText("打开文件夹…") || loadSettings().Root != "" {
		t.Fatal("workspace did not close and return to the empty sidebar")
	}
	if !tr.DropFiles(80, 240, root) || a.root != root || !tr.HasText("Page.md") {
		t.Fatal("dropping a directory did not open its tree")
	}
}

func TestFileTreeVisibilityRulesPersistAndFilterBothFilesAndDirectories(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	for _, dir := range []string{".private", "assets/images/icons", "assets/images/working/cached", "drafts"} {
		if err := os.MkdirAll(filepath.Join(root, dir), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	for _, name := range []string{
		"start.md", ".secret.md", ".private/saved.md", "assets/photo.png", "assets/song.wav", "drafts/other.md",
		"assets/images/icons/logo.png", "assets/images/icons/beep.wav", "assets/images/working/cached/draft.png",
	} {
		if err := os.WriteFile(filepath.Join(root, name), []byte("fixture"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(filepath.Join(root, "start.md"))
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	if a.tree.at(filepath.Join(root, ".secret.md")) != nil || a.tree.at(filepath.Join(root, ".private")) != nil ||
		a.tree.at(filepath.Join(root, "assets", "photo.png")) != nil {
		t.Fatal("the default filter exposed hidden or non-Markdown files")
	}
	workspaceTestClick(t, tr, "文件夹操作")
	if err := tr.ChooseMenuItem("筛选…"); err != nil {
		t.Fatal(err)
	}
	workspaceTestClick(t, tr, "显示隐藏文件和文件夹")
	workspaceTestClick(t, tr, "按规则显示")
	workspaceTestClick(t, tr, "显示匹配规则")
	tr.Type("*.md, assets/**/*.png")
	workspaceTestClick(t, tr, "隐藏匹配规则")
	tr.Type("[")
	workspaceTestClick(t, tr, "应用")
	if !a.treeFilterEdit.open || a.treeFilterEdit.err == "" {
		t.Fatal("invalid rule was accepted")
	}
	workspaceTestClick(t, tr, "隐藏匹配规则")
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Type("drafts, **/working/**")
	workspaceTestClick(t, tr, "应用")
	if a.treeFilterEdit.open || !tr.HasText(".secret.md") || !tr.HasText(".private") || tr.HasText("drafts") {
		t.Fatalf("unexpected filtered view: %q", tr.Texts())
	}
	workspaceTestClick(t, tr, "assets")
	if !tr.HasText("photo.png") || tr.HasText("song.wav") {
		t.Fatal("include rule failed to match only the selected extension and path")
	}
	workspaceTestClick(t, tr, "images")
	workspaceTestClick(t, tr, "icons")
	if !tr.HasText("logo.png") || tr.HasText("beep.wav") || tr.HasText("working") {
		t.Fatal("recursive globs did not match nested files and exclude whole directories")
	}
	workspaceTestClick(t, tr, "文件夹操作")
	if err := tr.ChooseMenuItem("筛选…"); err != nil {
		t.Fatal(err)
	}
	workspaceTestClick(t, tr, "所有文件")
	workspaceTestClick(t, tr, "应用")
	if !tr.HasText("song.wav") || New("").treeFilter.Mode != workspaceAllFiles {
		t.Fatal("showing non-Markdown files did not update the open tree and saved preference")
	}
}

func TestFileTreeDragAddsRelativeLinkInVisualAndSourceEditor(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	current := filepath.Join(root, "Main.md")
	target := filepath.Join(root, "资料 (1).md")
	for _, p := range []string{current, target} {
		if err := os.WriteFile(p, []byte("写在这里\n"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(current)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	row, ok := tr.Find("资料 (1).md")
	if !ok {
		t.Fatal("cannot find the drag source")
	}
	editor := a.bufs[a.core.Doc.Blocks[0].ID].bounds
	drag := func(x, y float32) {
		startX, startY := row.X+row.W/2, row.Y+row.H/2
		tr.Press(startX, startY)
		tr.Move(x, y)
		tr.Release(x, y)
	}
	drag(editor.X+5, editor.Y+editor.H/2)
	got := a.markdownSource()
	if !a.dirty || !strings.Contains(got, "[资料 (1).md]") || !strings.Contains(got, "%20") || a.treeEdit.open {
		t.Fatalf("visual drop failed: %q", got)
	}
	a.toggleSource()
	tr.Frame()
	before := a.source.text()
	x, y := workspaceSourceCaretPoint(t, tr, a.source, len(before))
	a.source.edit.Select(0, 0)
	drag(x, y)
	if after := a.source.text(); !strings.HasPrefix(after, before) || len(after) <= len(before) || !a.dirty {
		t.Fatalf("source drop failed: %q", after)
	}
}

func TestFileTreeMoveRebasesImageTargetsAndUndoRestoresOriginalFile(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	archive := filepath.Join(root, "Archive")
	pics := filepath.Join(root, "pics")
	for _, dir := range []string{archive, pics} {
		if err := os.Mkdir(dir, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	image := []byte(`<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>`)
	if err := os.WriteFile(filepath.Join(pics, "image 1.svg"), image, 0o600); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(root, "Page.md")
	original := []byte(utf8BOM + "![caption](<./pics/image 1.svg> 'title')\r\n\r\n" +
		"![ref][hero]\n\n[hero]: <pics/image%201.svg> \"ref title\"\r\n\r\n" +
		"`![example](pics/image.svg)`\r\n")
	if err := os.WriteFile(path, original, 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	a.toggleSource()
	s := a.source
	oldEnd := len(s.text())
	s.edit.Select(oldEnd, oldEnd)
	s.edit.Insert("未保存的内容")
	savedBeforeMove := a.file.text
	if err := a.moveTreeInto(a.tree.at(path), a.tree.at(archive)); err != nil {
		t.Fatal(err)
	}
	moved := filepath.Join(archive, "Page.md")
	if a.path != moved || a.source != s || !a.dirty || !strings.HasSuffix(s.text(), "未保存的内容") ||
		s.edit.Selection().Head != len(s.text()) || !strings.Contains(s.text(), "../pics/image%201.svg") {
		t.Fatalf("relocation did not preserve the live source and caret: %q, %q", a.path, s.text())
	}
	expected := bytes.ReplaceAll(original, []byte("./pics/image 1.svg"), []byte("../pics/image%201.svg"))
	expected = bytes.ReplaceAll(expected, []byte("[hero]: <pics/"), []byte("[hero]: <../pics/"))
	if actual, err := os.ReadFile(moved); err != nil || !bytes.Equal(expected, actual) {
		t.Fatalf("the saved file changed outside its image destinations: %q, %v", actual, err)
	}
	if a.savedText == savedBeforeMove || a.file.path != moved {
		t.Fatal("saved baseline did not follow the relocation")
	}
	if got, _, err := documentImageBytes(filepath.Dir(moved), "../pics/image%201.svg"); err != nil || !bytes.Equal(got, image) {
		t.Fatalf("rebased path did not load the original image: %v", err)
	}
	if err := a.undoTreeOperation(); err != nil {
		t.Fatal(err)
	}
	if actual, err := os.ReadFile(path); err != nil || !bytes.Equal(original, actual) || a.path != path ||
		a.source != s || !a.dirty || !strings.HasSuffix(s.text(), "未保存的内容") {
		t.Fatalf("undo did not restore the original bytes and detached the live text: %v, %q, %q", err, actual, s.text())
	}
}

func TestFileTreeFolderMoveKeepsBundledImagesAndRebasesSharedImages(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	prev, archive := filepath.Join(root, "Notes"), filepath.Join(root, "Archive")
	for _, name := range []string{prev, archive} {
		if err := os.Mkdir(name, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	path := filepath.Join(prev, "Page.md")
	original := "![inside](./pics/photo.svg)\n\n![outside](../shared.svg)\n"
	if err := os.WriteFile(path, []byte(original), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	a.openWorkspaceFolder(root)
	if err := a.moveTreeInto(a.tree.at(prev), a.tree.at(archive)); err != nil {
		t.Fatal(err)
	}
	expected := strings.ReplaceAll(original, "../shared.svg", "../../shared.svg")
	if a.path != filepath.Join(archive, "Notes", "Page.md") || a.dirty || a.markdownSource() != expected {
		t.Fatalf("moved directory changed intra-folder images or lost the shared path: %q, %v", a.markdownSource(), a.dirty)
	}
}
