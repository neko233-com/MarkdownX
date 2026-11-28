package app

import (
	"bytes"
	"errors"
	"image"
	"image/color"
	"image/draw"
	"image/jpeg"
	"image/png"
	"math"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"

	"github.com/neko233/markdownx/internal/diagram"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type diagramKey struct {
	language, source, font string
	dark                   bool
}

type diagramImage struct {
	vector *ui.SVG
	data   []byte
	err    error
}

func (a *App) diagramImage(language, source string, theme *ui.Theme) diagramImage {
	key := diagramKey{language, strings.TrimRight(source, "\r\n"), theme.Font, theme.Dark}
	if img, ok := a.diagramCache[key]; ok {
		return img
	}
	img := renderDiagram(key)
	if a.diagramCache == nil {
		a.diagramCache = make(map[diagramKey]diagramImage)
	}
	if len(a.diagramCache) >= 96 {
		clear(a.diagramCache)
	}
	a.diagramCache[key] = img
	return img
}

func renderDiagram(k diagramKey) diagramImage {
	data, err := diagram.Render(k.language, k.source, k.font, k.dark)
	if err != nil {
		return diagramImage{err: err}
	}
	svg, err := ui.ParseSVG(data)
	if err != nil {
		return diagramImage{err: err}
	}
	w, h := svg.Size()
	if w <= 0 || h <= 0 || w > 32768 || h > 32768 || math.IsNaN(float64(w+h)) {
		return diagramImage{err: errors.New("图表尺寸无效或过大")}
	}
	return diagramImage{vector: svg, data: data}
}

func (a *App) editableDiagram(c *ui.Context, b *doc.Block, language string) {
	a.sync(b)
	img := a.diagramImage(language, b.Text.Text, c.Theme())
	if a.focusReq == b.ID || (img.vector == nil && a.diagramClosed == b.ID) {
		a.diagramClosed, a.diagramPreviewFocus = 0, 0
	}
	editing := img.vector == nil || a.focusReq == b.ID || (a.focus == b.ID && a.diagramClosed != b.ID)
	if !editing {
		if buffer := a.bufs[b.ID]; buffer != nil {
			buffer.bounds = ui.Rect{}
		}
	}
	ui.Column(c.Key(b.ID)).Gap(8).Children(func() {
		if editing {
			a.codeEditor(c, b)
		}
		if img.vector == nil {
			diagramDiagnostic(c, img.err)
			return
		}
		e := a.drawDiagram(c, img, b.Text.Text)
		a.drawSelectedObject(c, e, b.ID)
		e.Role(ui.RoleButton).Focusable().Label("编辑图表源码").Cursor(ui.CursorPointer)
		if a.diagramPreviewFocus == b.ID {
			e.Focus()
			a.diagramPreviewFocus = 0
		}
		if e.Clicked() {
			if editing {
				a.closeDiagramSource(b.ID)
			} else {
				a.diagramClosed = 0
				a.focusAt(editor.Caret(editor.Pos{Block: b.ID, Offset: a.sel[b.ID][1]}))
			}
			a.services.Invalidate()
		}
	})
}

func (a *App) closeDiagramSource(id doc.BlockID) {
	a.diagramClosed, a.diagramPreviewFocus = id, id
}

func (a *App) previewCode(c *ui.Context, block *markdown.CodeBlockView) {
	lang := diagram.Language(block.Language)
	if !a.diagrams || lang == "" || block.Open {
		markdown.DefaultCodeBlock(c, block)
		return
	}
	img := a.diagramImage(lang, block.Code(), c.Theme())
	if img.vector == nil {
		markdown.DefaultCodeBlock(c, block)
		diagramDiagnostic(c, img.err)
		return
	}
	a.drawDiagram(c, img, block.Code()).Label("Markdown 图表")
}

func diagramDiagnostic(c *ui.Context, err error) {
	if err == nil {
		return
	}
	color := c.Theme().Danger
	if errors.Is(err, diagram.ErrEmpty) {
		color = c.Theme().TextMuted
	}
	ui.Text(c, err.Error()).FontSize(12).TextColor(color)
}

const diagramSVG transfer.Format = "image/svg+xml"

func (a *App) drawDiagram(c *ui.Context, img diagramImage, source string) ui.Element {
	w, h := img.vector.Size()
	width := min(w, 1440, 880*w/h)
	background := c.Theme().Background
	return ui.Image(c.Key("diagram"), img.vector).Width(width).MaxWidthPercent(100).
		Fit(ui.Contain).AlignSelf(ui.Center).ContextMenu(func(m *ui.Menu) {
		if m.Item("复制图表").Chosen() {
			a.copyDiagram(img)
		}
		if m.Item("复制图表源码").Chosen() {
			a.services.WriteClipboard(source)
		}
		m.Separator()
		for _, ext := range []string{"svg", "png", "jpg"} {
			if m.Item("保存为 " + strings.ToUpper(ext) + "…").Chosen() {
				a.saveDiagram(img, ext, background)
			}
		}
	})
}

func (a *App) copyDiagram(img diagramImage) {
	data, err := img.encode("png", ui.Color{})
	if err != nil {
		a.status = "复制图表失败：" + err.Error()
		return
	}
	a.services.WriteClipboardData(transfer.New(transfer.NewItem(
		transfer.Bytes(transfer.PNG, data), transfer.Bytes(diagramSVG, img.data),
	)))
	a.status = "已复制图表"
}

func (a *App) saveDiagram(img diagramImage, extension string, background ui.Color) {
	path, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
		Parent: a.win, Title: "导出图表",
		DefaultPath: "diagram." + extension,
		Filters:     []mygo.FileFilter{{Name: strings.ToUpper(extension), Extensions: []string{extension}}},
	})
	if err != nil || path == "" {
		return
	}
	ext := strings.ToLower(filepath.Ext(path))
	if ext != "."+extension && !(extension == "jpg" && ext == ".jpeg") {
		path += "." + extension
	}
	data, err := img.encode(extension, background)
	if err == nil {
		err = writeMarkdown(path, data, nil, false)
	}
	if err != nil {
		a.status = "导出图表失败：" + err.Error()
		return
	}
	a.status = "已导出 " + filepath.Base(path)
}

func (img diagramImage) encode(format string, background ui.Color) ([]byte, error) {
	if format == "svg" {
		return img.data, nil
	}
	w, h := img.vector.Size()
	factor := min(2, 4096/max(w, h))
	pixels := img.vector.Image(max(1, int(math.Ceil(float64(w*factor)))), max(1, int(math.Ceil(float64(h*factor)))), nil)
	var b bytes.Buffer
	var err error
	if format == "jpg" {
		opaque := image.NewRGBA(pixels.Bounds())
		fill := image.NewUniform(color.NRGBA{R: background.R, G: background.G, B: background.B, A: 255})
		draw.Draw(opaque, opaque.Bounds(), fill, image.Point{}, draw.Src)
		draw.Draw(opaque, opaque.Bounds(), pixels, image.Point{}, draw.Over)
		err = jpeg.Encode(&b, opaque, &jpeg.Options{Quality: 92})
	} else {
		err = png.Encode(&b, pixels)
	}
	return b.Bytes(), err
}
