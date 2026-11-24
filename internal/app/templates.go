package app

import (
	"errors"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// template is one entry of the 模板参考 tab: a named Markdown snippet the
// user can insert into the open document.
type template struct {
	Name string
	Body string
	// Path is the file it was read from; empty for built-ins.
	Path string
}

// builtinTemplates are the samples shipped with the app.
func builtinTemplates() []template {
	return []template{
		{Name: "会议记录", Body: `# 会议记录

- 时间：
- 地点：
- 参与人：

## 议题

1. 

## 讨论

- 

## 结论

- 

## 待办

- [ ] 
`},
		{Name: "每周回顾", Body: `# 每周回顾

## 本周完成

- 

## 遇到的问题

- 

## 下周计划

- [ ] 
`},
		{Name: "读书笔记", Body: `# 书名

- 作者：
- 阅读时间：

## 摘要

> 

## 关键观点

1. 

## 我的想法

`},
		{Name: "项目计划", Body: `# 项目名称

## 目标

- 

## 里程碑

| 阶段 | 时间 | 产出 |
| --- | --- | --- |
|  |  |  |

## 风险

- 
`},
		{Name: "空白待办", Body: `# 待办

- [ ] 
`},
	}
}

// templatesDir is where the user's own templates live.
func templatesDir() string {
	dir, err := os.UserConfigDir()
	if err != nil || dir == "" {
		return ""
	}
	return filepath.Join(dir, "MarkdownX", "templates")
}

// loadTemplates lists the built-ins first, then the user's templates in
// name order.
func loadTemplates() []template {
	out := builtinTemplates()
	dir := templatesDir()
	if dir == "" {
		return out
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return out
	}
	var names []string
	for _, e := range entries {
		if !e.IsDir() && strings.EqualFold(filepath.Ext(e.Name()), ".md") {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)
	for _, name := range names {
		p := filepath.Join(dir, name)
		data, err := os.ReadFile(p)
		if err != nil {
			continue
		}
		out = append(out, template{
			Name: strings.TrimSuffix(name, filepath.Ext(name)),
			Body: string(data),
			Path: p,
		})
	}
	return out
}

// saveTemplate writes body as a user template and returns its path.
func saveTemplate(name, body string) (string, error) {
	dir := templatesDir()
	if dir == "" {
		return "", errors.New("找不到配置目录")
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return "", err
	}
	p := filepath.Join(dir, safeTemplateName(name)+".md")
	if err := os.WriteFile(p, []byte(body), 0o644); err != nil {
		return "", err
	}
	return p, nil
}

// safeTemplateName strips the characters a file name cannot hold, so a title
// like "周报 2026/11" still becomes a template instead of failing.
func safeTemplateName(name string) string {
	name = strings.Map(func(r rune) rune {
		switch r {
		case '/', '\\', ':', '*', '?', '"', '<', '>', '|':
			return '-'
		}
		return r
	}, strings.TrimSpace(name))
	name = strings.Trim(name, ". ")
	if name == "" {
		name = "未命名模板"
	}
	return name
}
