package app

import (
	"bytes"
	"image"
	"math"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/neko233/markdownx/internal/diagram"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/export/pdf"
	"golang.org/x/image/draw"
)

type pdfPreferences struct {
	Paper, Margin, Header, Footer string
	Landscape, BreakAtH1          bool
}

type pdfDialog struct {
	settings      pdfPreferences
	open, preview bool
	err           string
}

type pdfResult struct {
	path    string
	preview bool
	err     error
}

func (a *App) ExportPDF() { a.openPDFDialog(false) }

func (a *App) printPreview() { a.openPDFDialog(true) }

func (a *App) openPDFDialog(preview bool) {
	if a.pdfJob != nil {
		return
	}
	defaults := pdf.Defaults()
	d := pdfDialog{open: true, preview: preview, settings: pdfPreferences{
		Paper: defaults.Paper, Footer: defaults.Footer, Margin: strconv.FormatFloat(defaults.MarginMM, 'f', -1, 64),
	}}
	if saved := loadSettings().PDF; saved != nil {
		d.settings = *saved
	}
	a.pdfDialog = d
}

func (a *App) showPDFDialog(c *ui.Context) {
	d := &a.pdfDialog
	if !d.open {
		return
	}
	ui.Modal(c, &d.open, func() {
		ui.Column(c).Width(405).Padding(20).Gap(10).Background(c.Theme().Surface).Radius(10).Children(func() {
			caption, submit := "导出 PDF", "选择保存位置…"
			if d.preview {
				caption, submit = "打印预览", "保存并打开…"
			}
			ui.Text(c, caption).FontSize(18).Bold()
			ui.Row(c).Gap(12).AlignItems(ui.Center).Children(func() {
				ui.Text(c, "纸张").Width(86)
				ui.Select(c.Key("pdf-paper"), &d.settings.Paper, []string{"A4", "A5", "Letter", "Legal"}).Label("纸张")
				ui.Checkbox(c.Key("pdf-landscape"), &d.settings.Landscape, "横向")
			})
			pdfTextField(c, "页边距 (mm)", &d.settings.Margin)
			pdfTextField(c, "页眉", &d.settings.Header)
			pdfTextField(c, "页脚", &d.settings.Footer)
			ui.Checkbox(c.Key("pdf-page-break"), &d.settings.BreakAtH1, "一级标题之间分页")
			if d.err != "" {
				ui.Text(c, d.err).TextColor(c.Theme().Danger)
			}
			ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
				if ui.Button(c.Key("pdf-cancel"), "取消").Clicked() {
					d.open = false
				}
				if ui.Button(c.Key("pdf-save"), submit).Clicked() {
					a.choosePDFFile()
				}
			})
		})
	})
}

func pdfTextField(c *ui.Context, label string, text *string) {
	ui.Row(c).Gap(12).AlignItems(ui.Center).Children(func() {
		ui.Text(c, label).Width(86)
		ui.TextInput(c.Key(label), text).Label(label).Grow(1)
	})
}

func (a *App) choosePDFFile() {
	d := &a.pdfDialog
	p := d.settings
	margin, err := strconv.ParseFloat(strings.TrimSpace(p.Margin), 64)
	if err != nil || math.IsNaN(margin) || math.IsInf(margin, 0) || margin < 5 || margin > 50 {
		d.err = "页边距须在 5 到 50 毫米之间。"
		return
	}
	d.err = ""
	name := a.DisplayName()
	if a.path != "" {
		name = strings.TrimSuffix(filepath.Base(a.path), filepath.Ext(a.path))
	}
	path := firstNonEmpty(name, "document") + ".pdf"
	if a.path != "" {
		path = filepath.Join(filepath.Dir(a.path), path)
		if samePath(a.path, path) {
			path = filepath.Join(filepath.Dir(a.path), name+"-export.pdf")
		}
	}
	out, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
		Parent: a.win, Title: "导出 PDF", DefaultPath: path,
		Filters: []mygo.FileFilter{{Name: "PDF", Extensions: []string{"pdf"}}},
	})
	if err != nil || out == "" {
		return
	}
	if !strings.EqualFold(filepath.Ext(out), ".pdf") {
		out += ".pdf"
	}
	if a.path != "" && samePath(a.path, out) {
		d.err = "请选择与原文档不同的文件名。"
		return
	}
	current := a.currentDocument().Clone()
	opts := pdf.Options{Paper: p.Paper, MarginMM: margin, Landscape: p.Landscape, BreakAtH1: p.BreakAtH1,
		Title: name, Header: p.Header, Footer: p.Footer,
	}
	dir := a.root
	if a.path != "" {
		dir = filepath.Dir(a.path)
	}
	opts.Media = pdfMedia(dir, a.diagrams)
	s := loadSettings()
	s.PDF = &p
	saveSettings(s)
	d.open = false
	a.startPDF(out, current, opts, d.preview)
}

func (a *App) startPDF(path string, d *doc.Document, opts pdf.Options, preview bool) {
	job := make(chan pdfResult, 1)
	a.pdfJob = job
	a.status = "正在生成 PDF…"
	service := a.services
	go func() {
		var output bytes.Buffer
		err := pdf.Write(&output, d, opts)
		if err == nil {
			err = writeMarkdown(path, output.Bytes(), nil, false)
		}
		job <- pdfResult{path, preview, err}
		service.Invalidate()
	}()
}

func (a *App) pollPDF() {
	select {
	case result := <-a.pdfJob:
		a.pdfJob = nil
		if result.err != nil {
			a.status = "导出 PDF 失败：" + result.err.Error()
			return
		}
		a.status = "已导出 " + filepath.Base(result.path)
		if result.preview {
			if err := mygo.Shell.OpenPath(result.path); err != nil {
				a.status += "，无法打开预览：" + err.Error()
			}
		} else {
			rememberExport("pdf")
		}
	default:
	}
}

func pdfMedia(dir string, diagrams bool) func(string, string, bool) image.Image {
	theme := ui.LightTheme()
	return func(kind, source string, display bool) image.Image {
		switch kind {
		case "image":
			return pdfImage(dir, source)
		case "math":
			size := float32(11)
			if display {
				size = 16
			}
			m := renderMath(mathKey{source: source, size: size, display: display, color: theme.Text})
			return pdfSVG(m.source, 2)
		default:
			if diagrams && diagram.Language(kind) != "" {
				art := renderDiagram(diagramKey{language: kind, source: source, font: theme.Font})
				return pdfSVG(art.vector, 1.5)
			}
		}
		return nil
	}
}

func pdfImage(dir, source string) image.Image {
	raw, vector, err := documentImageBytes(dir, source)
	if err != nil {
		return nil
	}
	if vector {
		img, _ := ui.ParseSVG(raw)
		return pdfSVG(img, 1)
	}
	config, _, err := image.DecodeConfig(bytes.NewReader(raw))
	if err != nil || config.Width <= 0 || config.Height <= 0 || int64(config.Width)*int64(config.Height) > 40_000_000 {
		return nil
	}
	img, _, err := image.Decode(bytes.NewReader(raw))
	if err != nil {
		return nil
	}
	if scale := 2200.0 / float64(max(config.Width, config.Height)); scale < 1 {
		dst := image.NewRGBA(image.Rect(0, 0, max(1, int(float64(config.Width)*scale)), max(1, int(float64(config.Height)*scale))))
		draw.CatmullRom.Scale(dst, dst.Bounds(), img, img.Bounds(), draw.Src, nil)
		return dst
	}
	return img
}

func pdfSVG(svg *ui.SVG, density float32) image.Image {
	if svg == nil {
		return nil
	}
	w, h := svg.Size()
	if w <= 0 || h <= 0 || math.IsNaN(float64(w+h)) || math.IsInf(float64(w+h), 0) {
		return nil
	}
	scale := min(density, 2200/max(w, h))
	return svg.Image(max(1, int(math.Ceil(float64(w*scale)))), max(1, int(math.Ceil(float64(h*scale)))), nil)
}
