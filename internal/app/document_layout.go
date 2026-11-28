package app

import (
	"embed"
	"sync"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
)

//go:embed fonts/OpenSans-*.ttf
var documentFonts embed.FS

var documentFont = sync.OnceValue(func() string {
	faces, _ := documentFonts.ReadDir("fonts")
	for _, face := range faces {
		data, err := documentFonts.ReadFile("fonts/" + face.Name())
		if err != nil || ui.RegisterFont(data, "Open Sans") != nil {
			return ""
		}
	}
	return "Open Sans"
})

const (
	documentFontSize  float32 = 16
	documentLeading   float32 = 1.6
	documentCodeScale float32 = 0.92
	inlineCodePadding float32 = 2
)

func documentInk(t *ui.Theme) ui.Color { return t.Text.Mix(t.Background, 0.12) }

func headingMetrics(level uint8) (size, leading float32) {
	switch level {
	case 1:
		return 36, 1.2
	case 2:
		return 28, 1.225
	case 3:
		return 24, 1.43
	case 4:
		return 20, 1.4
	default:
		return 16, 1.4
	}
}

func blockSpacing(before, after *doc.Block) float32 {
	if h, heading := before.Kind.(doc.HeadingKind); heading {
		if h.Level == 1 {
			return 12
		}
		if after.Kind.IsList() {
			return 14
		}
		return 16
	}
	if _, heading := after.Kind.(doc.HeadingKind); heading {
		return 14
	}
	if before.Kind.IsList() && after.Kind.IsList() {
		return 8
	}
	return 12.8
}

func (a *App) listItem(c *ui.Context, b *doc.Block, marker string) {
	ui.Row(c).AlignItems(ui.Start).Gap(7).Children(func() {
		if k, task := b.Kind.(doc.TodoKind); task {
			ui.Column(c).Width(23).Height(documentFontSize * documentLeading).Justify(ui.Center).AlignItems(ui.End).Children(func() {
				done := k.Done
				ui.Checkbox(c.Key(b.ID), &done, "").Label("已完成")
				if done != k.Done {
					a.core.ToggleTodoDone(b.ID)
					a.updateDirty()
				}
			})
		} else if marker == "•" {
			ui.Column(c).Width(23).Height(documentFontSize*documentLeading).Padding(0, 4, 0, 0).
				Justify(ui.Center).AlignItems(ui.End).Children(func() {
				ui.Box(c).Size(5, 5).Radius(3).Background(documentInk(c.Theme()))
			})
		} else {
			ui.Text(c, marker).SingleLine().MinWidth(23).Shrink(0).TextAlign(ui.End).Font(documentFont()).
				FontSize(documentFontSize).LineHeight(documentLeading).TextColor(documentInk(c.Theme()))
		}
		ui.Column(c).Grow(1).MinWidth(0).Children(func() {
			a.textArea(c, b, documentFontSize)
			a.children(c, b)
		})
	})
}
