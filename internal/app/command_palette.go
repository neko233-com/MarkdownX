package app

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

type commandPalette struct {
	open, focus bool
	query       string
	selected    int
}

func (a *App) openCommandPalette() {
	a.palette = commandPalette{open: true, focus: true}
}

func (a *App) paletteCommands(query string) []command {
	var matches []command
	words := strings.Fields(strings.ToLower(query))
	for _, cmd := range commands {
		if cmd.menu == "" || cmd.label == "命令面板…" || !a.commandAvailable(cmd) {
			continue
		}
		search := strings.ToLower(cmd.menu + " " + cmd.label + " " + cmd.shortcutLabel())
		if match, _ := matchQuickText(search, words); match {
			matches = append(matches, cmd)
		}
	}
	return matches
}

func (cmd command) shortcutLabel() string {
	primary := "Ctrl"
	if cmd.mods&ui.Super != 0 {
		primary = "Cmd"
	}
	return strings.ReplaceAll(cmd.accelerator, "CmdOrCtrl", primary)
}

func (a *App) commandPaletteDialog(c *ui.Context) {
	p := &a.palette
	if !p.open {
		return
	}
	var run func(*App)
	ui.Modal(c, &p.open, func() {
		t := c.Theme()
		ui.Column(c).Width(518).Padding(16).Gap(8).Background(t.Surface).Radius(t.Radius).Children(func() {
			input := ui.TextInput(c.Key("commands-query"), &p.query).Label("命令面板").Placeholder("搜索命令、菜单或快捷键")
			if p.focus {
				input.Focus()
				p.focus = false
			}
			if input.Changed() {
				p.selected = 0
			}
			hits := a.paletteCommands(p.query)
			p.selected = max(0, min(p.selected, len(hits)-1))
			input.OnTextKey(func(key ui.TextKey) bool {
				switch key.Key {
				case ui.KeyDown:
					p.selected = max(0, min(p.selected+1, len(hits)-1))
				case ui.KeyUp:
					p.selected = max(0, p.selected-1)
				case ui.KeyEnter:
					if len(hits) > 0 {
						p.open = false
						hits[p.selected].run(a)
					}
				default:
					return false
				}
				return true
			})
			ui.Scroll(c.Key("commands-matches")).Height(292).Children(func() {
				ui.Column(c).Gap(2).Children(func() {
					if len(hits) == 0 {
						ui.Text(c, "没有匹配的命令").TextColor(t.TextMuted)
					}
					for i, cmd := range hits {
						row := ui.ButtonBase(c.Key([2]string{cmd.menu, cmd.label})).Height(31).
							Padding(0, 8).FillWidth().Label(cmd.label)
						if p.selected == i {
							row.Background(t.Selection).ScrollIntoView()
						}
						row.Children(func() {
							ui.Text(c, cmd.label).Grow(1).MinWidth(0).SingleLine().TruncateMiddle()
							if key := cmd.shortcutLabel(); key != "" {
								ui.Text(c, key).FontSize(11).TextColor(t.TextMuted).Shrink(0)
							}
						})
						if row.Clicked() {
							p.open = false
							run = cmd.run
						}
					}
				})
			})
		})
	})
	if run != nil {
		run(a)
		c.Invalidate()
	}
}
