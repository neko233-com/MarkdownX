package app

import (
	"path/filepath"
	"runtime"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type workspaceAction uint8

const (
	workspaceNewDocument workspaceAction = iota
	workspaceNewFolder
	workspaceRename
	workspaceTrash
	workspaceNewTemplate
)

type workspaceDialog struct {
	open, focus    bool
	action         workspaceAction
	node, dir      *workspaceNode
	name, err      string
	templates      map[string]workspaceTemplate
	templateList   []string
	templateChoice string
	suggestedName  string
	templateValues templateValues
}

func (a *App) beginTreeAction(action workspaceAction, node, dir *workspaceNode) {
	if err := a.checkTreeNode(node); err != nil {
		a.treeActionStatus(err)
		return
	}
	if dir != nil && dir != node {
		if err := a.checkTreeNode(dir); err != nil {
			a.treeActionStatus(err)
			return
		}
	}
	a.treeEdit = workspaceDialog{open: true, focus: true, action: action, node: node, dir: dir}
	if action == workspaceRename {
		a.treeEdit.name = node.name()
	}
	if action == workspaceNewTemplate {
		a.treeEdit.loadTemplates(templateValues{now: time.Now(), readClipboard: a.services.ReadClipboard})
	}
	a.services.Invalidate()
}

func (a *App) treeFileMenu(menu *ui.Menu, node *workspaceNode) {
	dir := node
	if !node.isDir() {
		dir = a.tree.at(filepath.Dir(node.path))
	}
	if dir != nil {
		a.treeCreateMenu(menu, node, dir)
	}
	if !sameWorkspacePath(node.path, a.root) {
		if menu.Item("重命名…").Shortcut(0, ui.KeyF2).Chosen() {
			a.beginTreeAction(workspaceRename, node, nil)
		}
		if a.win != nil && menu.Item("移动到…").Chosen() {
			a.chooseTreeDestination(node)
		}
		if menu.Item("制作副本").Chosen() {
			a.treeActionStatus(a.duplicateTreeEntry(node))
		}
		mods, key := treeTrashKey()
		if menu.Item("移到"+workspaceTrashLabel()+"…").Shortcut(mods, key).Chosen() {
			a.beginTreeAction(workspaceTrash, node, nil)
		}
	}
	if menu.Item("撤销上次文件操作").Disabled(!a.treeUndo.available()).Chosen() {
		a.treeActionStatus(a.undoTreeOperation())
	}
}

func (a *App) treeCreateMenu(menu *ui.Menu, node, dir *workspaceNode) {
	if menu.Item("新建文件…").Chosen() {
		a.beginTreeAction(workspaceNewDocument, node, dir)
	}
	if menu.Item("新建文件夹…").Chosen() {
		a.beginTreeAction(workspaceNewFolder, node, dir)
	}
	if menu.Item("从模板新建…").Chosen() {
		a.beginTreeAction(workspaceNewTemplate, node, dir)
	}
}

func treeTrashKey() (ui.Modifiers, ui.Key) {
	if runtime.GOOS == "darwin" {
		return ui.Super, ui.KeyBackspace
	}
	return 0, ui.KeyDelete
}

func (a *App) treeActionStatus(err error) {
	if err != nil {
		a.status = "文件操作失败：" + err.Error()
		a.services.Invalidate()
	}
}

func (a *App) treeActionKeys(tree ui.Element, node *workspaceNode) {
	if node == nil || sameWorkspacePath(node.path, a.root) {
		return
	}
	if tree.Shortcut(0, ui.KeyF2) {
		a.beginTreeAction(workspaceRename, node, nil)
	}
	if tree.Shortcut(treeTrashKey()) {
		a.beginTreeAction(workspaceTrash, node, nil)
	}
}

func (a *App) chooseTreeDestination(node *workspaceNode) {
	paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
		Parent: a.win, Title: "移动到文件夹", Directory: true, DefaultPath: filepath.Dir(node.path),
	})
	if err != nil || len(paths) == 0 {
		a.treeActionStatus(err)
		return
	}
	if err := a.checkTreeNode(node); err != nil {
		a.treeActionStatus(err)
		return
	}
	destination, err := filepath.EvalSymlinks(paths[0])
	if err != nil {
		a.treeActionStatus(err)
		return
	}
	path, err := filepath.EvalSymlinks(node.path)
	if err != nil {
		a.treeActionStatus(err)
		return
	}
	if _, inside := workspaceRel(path, destination); inside || sameWorkspacePath(filepath.Dir(path), destination) {
		a.status = "请选择另一个目标文件夹"
		return
	}
	to := filepath.Join(destination, node.name())
	a.treeActionStatus(a.moveTreeEntry(node, to, "已移动 "+node.name()))
}

func (a *App) workspaceDialog(c *ui.Context) {
	d := &a.treeEdit
	if !d.open {
		if d.node != nil {
			a.focusTreeEntry(d.node.path)
			*d = workspaceDialog{}
		}
		return
	}
	titles := [...]string{"新建文件", "新建文件夹", "重命名", "移到" + workspaceTrashLabel(), "从模板新建"}
	title := titles[d.action]
	ui.Modal(c.Key("workspace-dialog"), &d.open, func() {
		ui.Column(c).Width(360).Padding(16).Gap(10).Radius(10).Background(c.Theme().Surface).Children(func() {
			ui.Text(c, title).Bold()
			switch d.action {
			case workspaceTrash:
				ui.Text(c, "将 "+d.node.name()+" 移到系统"+workspaceTrashLabel()+"？").Wrap()
			default:
				label := "名称"
				if d.action == workspaceNewDocument {
					label = "文件名"
				}
				if d.action == workspaceNewTemplate {
					d.templatePicker(c)
					label = "文件名"
					ui.Text(c, label).FontSize(12)
				}
				field := ui.TextInput(c.Key("workspace-entry-name"), &d.name).Label(label).Placeholder(label)
				if d.focus {
					field.Focus().SetTextSelection(0, utf8.RuneCountInString(d.name))
					d.focus = false
				}
				field.OnTextKey(func(key ui.TextKey) bool {
					if key.Key == ui.KeyEnter && d.open {
						a.applyTreeAction()
						return true
					}
					return false
				})
			}
			if d.err != "" {
				ui.Text(c, d.err).Wrap().TextColor(c.Theme().TextMuted)
			}
			ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
				cancel := ui.Button(c, "取消")
				if d.focus {
					cancel.Focus()
					d.focus = false
				}
				if cancel.Clicked() {
					d.open = false
				}
				confirm := "确认"
				if d.action == workspaceNewTemplate {
					confirm = "创建"
				}
				if ui.PrimaryButton(c, confirm).Clicked() {
					a.applyTreeAction()
				}
			})
		})
	})
}

func (a *App) applyTreeAction() {
	d := &a.treeEdit
	var err error
	switch d.action {
	case workspaceNewDocument, workspaceNewFolder:
		_, err = a.createTreeEntry(d.dir, d.name, d.action == workspaceNewFolder, "")
	case workspaceNewTemplate:
		err = a.createWorkspaceTemplate(d)
	case workspaceRename:
		err = a.renameTreeEntry(d.node, d.name)
	case workspaceTrash:
		err = a.trashTreeEntry(d.node)
	}
	if err != nil {
		d.err = err.Error()
		a.services.Invalidate()
		return
	}
	*d = workspaceDialog{}
	a.services.Invalidate()
}

func (d *workspaceDialog) loadTemplates(values templateValues) {
	d.templateValues = values
	d.templates = make(map[string]workspaceTemplate)
	for _, tpl := range loadTemplates() {
		label := "内置 · " + tpl.Name
		if tpl.Path != "" {
			label = "我的模板 · " + filepath.Base(tpl.Path)
		}
		d.templates[label] = prepareWorkspaceTemplate(tpl)
		d.templateList = append(d.templateList, label)
	}
	d.templateChoice = d.templateList[0]
	d.suggestedName = d.templates[d.templateChoice].suggestedName(&d.templateValues)
	d.name = d.suggestedName
}

func (d *workspaceDialog) templatePicker(c *ui.Context) {
	ui.Text(c, d.dir.path).SingleLine().TruncateMiddle().Tooltip(d.dir.path).FontSize(11).TextColor(c.Theme().TextMuted)
	picker := ui.Select(c.Key("workspace-template"), &d.templateChoice, d.templateList).Label("选择模板").WidthPercent(100)
	if d.focus {
		picker.Focus()
		d.focus = false
	}
	choice := d.templates[d.templateChoice]
	if picker.Changed() {
		name := choice.suggestedName(&d.templateValues)
		if d.name == d.suggestedName {
			d.name = name
		}
		d.suggestedName = name
	}
	if choice.description != "" {
		ui.Text(c, choice.description).MaxLines(3).FontSize(11).TextColor(c.Theme().TextMuted)
	}
}
