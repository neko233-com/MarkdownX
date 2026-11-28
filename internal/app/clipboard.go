package app

import (
	"runtime"
	"strings"

	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/neko233/markdownx/internal/convert"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/export"
)

const markdownClipboard transfer.Format = "application/x-markdownx-markdown"

func (a *App) copyDocumentRange(cut bool) {
	d := a.core.SelectedDocument()
	if !a.core.Sel.AcrossBlocks() && len(d.Blocks) == 1 {
		d.Blocks[0].Kind = doc.TextKind{}
	}
	plain := clipboardText(d)
	source, _ := d.WorkspaceMarkdown()
	rtf := []byte(export.RichTextFormat(d))
	formats := []transfer.Representation{
		transfer.Bytes(transfer.HTML, []byte(export.FragmentWithDefinitions(d, a.core.Doc))),
		transfer.Bytes(transfer.Text, []byte(plain)),
		transfer.Bytes(markdownClipboard, []byte(source)),
		transfer.Bytes(richtext.RTF, rtf),
	}
	if runtime.GOOS == "windows" {
		formats = append(formats, transfer.Bytes("Rich Text Format", rtf))
	}
	a.services.WriteClipboardData(transfer.New(transfer.NewItem(formats...)))
	if cut {
		a.core.ReplaceSelection(doc.RichText{})
		a.ResetBuffers()
		a.focusAt(a.core.Sel)
	}
}

func hasReference(t doc.RichText) bool {
	for _, run := range t.Runs {
		if _, ok := run.Marks.Inline.Reference(); ok {
			return true
		}
	}
	return false
}

// clipboardText keeps paragraph breaks and uses tabs between table cells.
func clipboardText(d *doc.Document) string {
	var lines []string
	if !d.Title.IsEmpty() {
		lines = append(lines, d.Title.PlainText())
	}
	var walk func([]*doc.Block, []string) []string
	walk = func(blocks []*doc.Block, out []string) []string {
		for _, b := range blocks {
			if b.Kind.HasText() {
				out = append(out, b.Text.PlainText())
			}
			if table, ok := b.Kind.(doc.TableKind); ok && table.Table != nil {
				for _, row := range table.Table.Rows {
					cells := make([]string, len(row))
					for col, cell := range row {
						cells[col] = strings.Join(walk(cell.Blocks, nil), " ")
					}
					out = append(out, strings.Join(cells, "\t"))
				}
			}
			out = walk(b.Children, out)
		}
		return out
	}
	return strings.Join(walk(d.Blocks, lines), "\n")
}

// pasteDocumentBlocks lets the editor own block boundaries. A clipboard
// with only inline styling goes through the native rich-text widget.
func (a *App) pasteDocumentBlocks() bool {
	data := a.services.ReadClipboardData(markdownClipboard, transfer.HTML, transfer.Markdown)
	var md string
	if source, err := data.Read(markdownClipboard); err == nil {
		md = string(source)
	} else if html, err := data.Read(transfer.HTML); err == nil {
		md, _ = convert.HTML(html)
	} else if source, err := data.Read(transfer.Markdown); err == nil {
		md = string(source)
	}
	if md == "" {
		return false
	}
	blocks := doc.MarkdownToBlocks(md, doc.Off{File: true})
	if len(blocks) == 1 {
		if _, plain := blocks[0].Kind.(doc.TextKind); plain && len(blocks[0].Children) == 0 && !hasReference(blocks[0].Text) {
			return false
		}
	}
	return a.editFocused(func(*doc.Block) bool { return a.core.PasteBlocks(blocks) })
}
