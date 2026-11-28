package app

import (
	"fmt"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/junosuarez/termtex/pkg/tex"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type mathKey struct {
	source  string
	color   ui.Color
	size    float32
	display bool
}

type mathImage struct {
	source        *ui.SVG
	width, ascent float32
	descent       float32
}

func (a *App) mathImage(src string, size float32, display bool, color ui.Color) mathImage {
	key := mathKey{src, color, size, display}
	if img, ok := a.mathCache[key]; ok {
		return img
	}
	img := renderMath(key)
	if len(a.mathCache) >= 512 {
		clear(a.mathCache)
	}
	a.mathCache[key] = img
	return img
}

func renderMath(k mathKey) (out mathImage) {
	if len(k.source) == 0 || len(k.source) > 8192 {
		return mathImage{}
	}
	// Some TeX constructs can fail during layout even after parsing.
	defer func() {
		if recover() != nil {
			out = mathImage{}
		}
	}()
	ast, err := tex.Parse(k.source)
	if err != nil {
		return mathImage{}
	}
	opts := tex.DefaultRenderOptions()
	opts.FgColor = fmt.Sprintf("#%02x%02x%02x", k.color.R, k.color.G, k.color.B)
	opts.FontSize, opts.DisplayMode, opts.Padding = float64(k.size), k.display, 1
	page := tex.BuildPage(ast, opts)
	if len(page.Prims) == 0 || len(page.Prims) > 8192 ||
		page.Width <= 0 || page.Height <= 0 || page.Width > 8192 || page.Height > 8192 ||
		math.IsNaN(page.Width+page.Height+page.MinY) || math.IsInf(page.MinY, 0) {
		return mathImage{}
	}
	raw, err := tex.RenderSVG(ast, opts)
	if err != nil || len(raw) > 2<<20 {
		return mathImage{}
	}
	svg, err := ui.ParseSVG([]byte(raw))
	if err != nil {
		return mathImage{}
	}
	h := min(page.Height, max(0, -page.MinY))
	return mathImage{svg, float32(page.Width), float32(h), float32(page.Height - h)}
}

func (a *App) previewMath(c *ui.Context, src string) {
	img := a.mathImage(src, 18, true, c.Theme().Text)
	if img.source == nil {
		ui.Text(c, src).Font("monospace").TextColor(c.Theme().TextMuted)
		return
	}
	ui.Image(c, img.source).Width(img.width).MaxWidthPercent(100).
		AlignSelf(ui.Center).Label(src)
}

func (a *App) previewInlineMath(src string, size float32) (markdown.InlineObject, bool) {
	img := a.mathImage(src, size, false, a.inlineColor)
	if img.source == nil {
		return markdown.InlineObject{}, false
	}
	return markdown.InlineObject{
		Width: img.width, Height: img.ascent, Depth: img.descent,
		Draw: func(p *ui.Painter, r ui.Rect) { p.Image(img.source, r, ui.Contain) },
	}, true
}

type equationDialog struct {
	open, focus  bool
	replace      doc.BlockID
	newParagraph bool
	source       string
}

func displayEquation(b *doc.Block) (string, bool) {
	if b.Text.Text == doc.Obj && len(b.Text.Runs) == 1 {
		in := b.Text.Runs[0].Marks.Inline
		if in != nil && in.IsDisplayEquation() {
			return in.Equation()
		}
	}
	return "", false
}

func (a *App) openEquation() {
	if b := a.focused(); b != nil {
		a.openEquationFor(b)
	}
}

func (a *App) openEquationFor(b *doc.Block) {
	a.equation = equationDialog{open: true, focus: true}
	if src, ok := displayEquation(b); ok {
		a.equation.source, a.equation.replace = src, b.ID
	} else if b.Text.Text == "$$" || b.Text.Text == `\[` {
		a.equation.replace, a.equation.newParagraph = b.ID, true
	}
}

func (a *App) applyEquation() {
	d := &a.equation
	if strings.TrimSpace(d.source) == "" {
		return
	}
	in := doc.DisplayEquationInline(d.source)
	var text doc.RichText
	text.Push(doc.Obj, doc.Marks{Inline: &in})
	b := doc.NewBlock(doc.TextKind{}, text)
	b.Align = doc.AlignCenter
	a.syncAll()
	if d.replace != 0 {
		a.afterFormat(a.core.ReplaceBlock(d.replace, b, d.newParagraph))
	} else {
		target := a.focused()
		if target == nil {
			return
		}
		id := target.ID
		if cell, ok := a.core.TableCellAt(id); ok {
			id = cell.Block
		}
		a.core.InsertBlocks(id, b, doc.TextBlock(""))
		a.afterFormat(editor.Pos{Block: a.core.Sel.Head.Block})
	}
	d.open = false
}

func (a *App) mathDialog(c *ui.Context) {
	d := &a.equation
	if !d.open {
		return
	}
	t := c.Theme()
	ui.Modal(c, &d.open, func() {
		ui.Column(c).Width(440).Padding(20).Gap(12).Background(t.Surface).Radius(10).Children(func() {
			ui.Text(c, "公式块").FontSize(18).Bold()
			source := ui.TextArea(c.Key("math-source"), &d.source).Lines(3, 7).Font("monospace").Label("LaTeX 源码")
			if d.focus {
				source.Focus()
				d.focus = false
			}
			source.OnTextKey(func(k ui.TextKey) bool {
				if k.Key == ui.KeyEnter && k.Mods == ui.Cmd {
					d.source = k.Text()
					a.applyEquation()
					return true
				}
				return false
			})
			if strings.TrimSpace(d.source) != "" {
				a.previewMath(c, d.source)
			}
			ui.Row(c).Gap(8).Children(func() {
				ui.Spacer(c).Grow(1)
				if ui.Button(c.Key("math-cancel"), "取消").Clicked() {
					d.open = false
				}
				if ui.PrimaryButton(c.Key("math-confirm"), "应用公式").Clicked() {
					a.applyEquation()
				}
			})
		})
	})
}
