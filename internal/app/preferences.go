package app

import (
	"slices"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

func (a *App) preferences(c *ui.Context) {
	if !a.prefsOpen {
		return
	}
	t := c.Theme()
	ui.Modal(c, &a.prefsOpen, func() {
		_, height := c.Size()
		ui.Column(c).Width(360).Height(min(470, height-80)).Padding(16).Gap(10).Background(t.Surface).Radius(10).Children(func() {
			ui.Text(c, "偏好设置").FontSize(18).Bold()
			ui.Scroll(c.Key("preferences-scroll")).Grow(1).FillWidth().MinHeight(0).Children(func() {
				ui.Column(c).Gap(10).Children(func() {
					a.workspaceFilePreferences(c)
					if ui.Checkbox(c.Key("auto-save"), &a.autoSaveOn, "自动保存 Markdown").Changed() {
						a.remember()
					}
					if ui.Checkbox(c.Key("focus-mode"), &a.focusMode, "专注模式").Changed() {
						a.remember()
					}
					if ui.Checkbox(c.Key("typewriter"), &a.typeMode, "打字机模式").Changed() {
						a.reveal = a.focus
						a.remember()
					}
					if ui.Checkbox(c.Key("diagrams"), &a.diagrams, "渲染 Markdown 图表").Changed() {
						if a.diagrams {
							a.diagramClosed = 0
						}
						a.remember()
					}
				})
			})
			if ui.Button(c.Key("preferences-close"), "完成").AlignSelf(ui.End).Clicked() {
				a.prefsOpen = false
			}
		})
	})
}

func (a *App) workspaceFilePreferences(c *ui.Context) {
	s := loadSettings()
	ui.Field(c, "目录树默认文件类型", func() {
		if ui.Select(c.Key("workspace-document-extension"), &s.TreeDocumentExt, workspaceDocumentExtensions[:]).
			Label("目录树默认文件类型").FillWidth().Changed() {
			saveSettings(s)
		}
	})
	options := [...]string{"新建文档", "恢复上次的文件夹", "恢复上次的文档和文件夹"}
	choice := options[s.OnLaunch-workspaceLaunchNew]
	ui.Field(c, "启动时", func() {
		if ui.Select(c.Key("workspace-launch"), &choice, options[:]).Label("启动时").FillWidth().Changed() {
			s.OnLaunch = workspaceLaunch(slices.Index(options[:], choice)) + workspaceLaunchNew
			saveSettings(s)
		}
	})
	if s.OnLaunch != workspaceLaunchNew {
		return
	}
	ui.Field(c, "默认启动文件夹", func() {
		if s.DefaultWorkspace != "" {
			ui.Text(c, s.DefaultWorkspace).SingleLine().TruncateMiddle().Tooltip(s.DefaultWorkspace).
				FontSize(12).TextColor(c.Theme().TextMuted)
		}
		ui.Row(c).Gap(8).Children(func() {
			if ui.Button(c, "选择文件夹…").Clicked() {
				paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
					Parent: a.win, Title: "选择默认启动文件夹", Directory: true,
					DefaultPath: firstNonEmpty(s.DefaultWorkspace, a.root),
				})
				if err == nil && len(paths) > 0 {
					if path, ok := workspaceFolderPath(paths[0]); ok {
						s.DefaultWorkspace = path
						saveSettings(s)
					}
				}
			}
			if s.DefaultWorkspace != "" && ui.Button(c, "清除").Label("清除默认启动文件夹").Clicked() {
				s.DefaultWorkspace = ""
				saveSettings(s)
			}
		})
	})
}
