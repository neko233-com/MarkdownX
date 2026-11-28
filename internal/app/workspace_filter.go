package app

import (
	"fmt"
	"path"
	"path/filepath"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/convert"
)

type workspaceFilterMode uint8

const (
	workspaceSupportedFiles workspaceFilterMode = iota
	workspaceAllFiles
	workspaceCustomFiles
)

type workspaceFilter struct {
	ShowHidden bool                `json:"show_hidden,omitempty"`
	Mode       workspaceFilterMode `json:"mode,omitempty"`
	Include    string              `json:"include,omitempty"`
	Exclude    string              `json:"exclude,omitempty"`
}

type workspaceRules struct {
	workspaceFilter
	include, exclude []workspacePattern
}

type workspacePattern struct {
	glob      string
	relative  bool
	recursive []string
}

func workspacePatterns(value string) []workspacePattern {
	var patterns []workspacePattern
	for _, part := range strings.FieldsFunc(value, func(r rune) bool { return r == ',' || r == '，' || r == '\n' }) {
		glob := workspacePathKey(strings.TrimSpace(part))
		segments := strings.Split(glob, "/")
		p := workspacePattern{glob: glob, relative: len(segments) > 1}
		if p.relative && slices.Contains(segments, "**") {
			p.recursive = segments
		}
		patterns = append(patterns, p)
	}
	return patterns
}

func (filter workspaceFilter) rules() workspaceRules {
	return workspaceRules{filter, workspacePatterns(filter.Include), workspacePatterns(filter.Exclude)}
}

func (rules workspaceRules) validate() error {
	groups := [][]workspacePattern{rules.exclude}
	if rules.Mode == workspaceCustomFiles {
		groups = append(groups, rules.include)
	}
	for _, group := range groups {
		for _, pattern := range group {
			if _, err := path.Match(pattern.glob, ""); err != nil {
				return fmt.Errorf("无效的筛选规则 %q", pattern.glob)
			}
		}
	}
	return nil
}

func matchesWorkspacePattern(patterns []workspacePattern, name, relative string) bool {
	for _, pattern := range patterns {
		target := workspacePathKey(name)
		if pattern.relative {
			target = workspacePathKey(filepath.ToSlash(relative))
		}
		if pattern.match(target) {
			return true
		}
	}
	return false
}

func (pattern workspacePattern) match(target string) bool {
	if len(pattern.recursive) == 0 {
		match, _ := path.Match(pattern.glob, target)
		return match
	}
	segments, components := pattern.recursive, strings.Split(target, "/")
	part, component := 0, 0
	lastStar, consumed := -1, 0
	for component < len(components) {
		if part < len(segments) {
			if segments[part] == "**" {
				// Try zero components first, then extend this wildcard on a mismatch.
				lastStar, consumed = part, component
				part++
				continue
			}
			if match, _ := path.Match(segments[part], components[component]); match {
				part++
				component++
				continue
			}
		}
		if lastStar < 0 {
			return false
		}
		consumed++
		part, component = lastStar+1, consumed
	}
	for part < len(segments) && segments[part] == "**" {
		part++
	}
	return part == len(segments)
}

func (rules workspaceRules) pruneDirectory(name, relative string) bool {
	return strings.HasPrefix(name, ".markdownx-copy-") ||
		!rules.ShowHidden && strings.HasPrefix(name, ".") ||
		matchesWorkspacePattern(rules.exclude, name, relative)
}

func (rules workspaceRules) showFile(name, relative string) bool {
	if strings.HasPrefix(name, ".markdownx-") && strings.HasSuffix(name, ".tmp") ||
		!rules.ShowHidden && strings.HasPrefix(name, ".") ||
		matchesWorkspacePattern(rules.exclude, name, relative) {
		return false
	}
	switch rules.Mode {
	case workspaceAllFiles:
		return true
	case workspaceCustomFiles:
		return matchesWorkspacePattern(rules.include, name, relative)
	default:
		format := convert.Detect(name)
		return format == convert.FormatMarkdown || format == convert.FormatMdxb
	}
}

type workspaceFilterDialog struct {
	open  bool
	value workspaceFilter
	err   string
}

func (a *App) workspaceFilterDialog(c *ui.Context) {
	d := &a.treeFilterEdit
	if !d.open {
		return
	}
	ui.Modal(c.Key("workspace-filters"), &d.open, func() {
		t := c.Theme()
		ui.Column(c).Width(400).Padding(16).Gap(10).Radius(10).Background(t.Surface).Children(func() {
			ui.Text(c, "文件树筛选").Bold()
			ui.Checkbox(c.Key("show-hidden"), &d.value.ShowHidden, "显示隐藏文件和文件夹")
			ui.RadioGroup(c.Key("workspace-file-types"), func() {
				ui.Row(c).Gap(12).Children(func() {
					ui.Radio(c, &d.value.Mode, workspaceSupportedFiles, "支持的文档")
					ui.Radio(c, &d.value.Mode, workspaceAllFiles, "所有文件")
					ui.Radio(c, &d.value.Mode, workspaceCustomFiles, "按规则显示")
				})
			})
			if d.value.Mode == workspaceCustomFiles {
				ui.Field(c, "显示匹配项", func() {
					ui.TextInput(c.Key("tree-include"), &d.value.Include).Label("显示匹配规则").Placeholder("例如 *.md, assets/**/*.svg")
				})
			}
			ui.Field(c, "隐藏匹配项", func() {
				ui.TextInput(c.Key("tree-exclude"), &d.value.Exclude).Label("隐藏匹配规则").Placeholder("例如 node_modules, *.bak")
			})
			if d.err != "" {
				ui.Text(c, d.err).Wrap().TextColor(t.TextMuted)
			}
			ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
				if ui.Button(c, "取消").Clicked() {
					d.open = false
				}
				if ui.Button(c, "应用").Clicked() {
					if err := d.value.rules().validate(); err != nil {
						d.err = err.Error()
						return
					}
					a.applyWorkspaceFilter(d.value)
					*d = workspaceFilterDialog{}
				}
			})
		})
	})
}

func (a *App) applyWorkspaceFilter(filter workspaceFilter) {
	s := loadSettings()
	s.TreeFilter = filter
	saveSettings(s)
	for _, w := range a.workspaceWindows() {
		w.treeFilter = filter
		w.stopWorkspaceWatch()
		w.refreshFiles()
		w.services.Invalidate()
	}
	a.showWorkspaceTree()
}
