package convert

import (
	"os"
	"path/filepath"
	"testing"
)

// TestDemoDocuments converts the sample documents that ship with the project.
// It skips when the folder is not part of the checkout.
func TestDemoDocuments(t *testing.T) {
	root := filepath.Join("..", "..", "MarkdownX-Demo")
	if _, err := os.Stat(root); err != nil {
		t.Skip("demo folder not present")
	}
	supported := map[string]bool{
		".docx": true, ".xlsx": true, ".epub": true,
		".ofd": true, ".pdf": true, ".rtf": true, ".html": true,
	}
	checked := 0
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return nil
		}
		if !supported[filepath.Ext(path)] {
			return nil
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		md, err := Markdown(path, data)
		if err != nil {
			t.Errorf("%s: %v", filepath.Base(path), err)
			return nil
		}
		if len(md) < 10 {
			t.Errorf("%s: output too short (%d bytes)", filepath.Base(path), len(md))
		}
		checked++
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if checked == 0 {
		t.Skip("no demo documents found")
	}
	t.Logf("converted %d demo documents", checked)
}
