package diagram

import (
	"fmt"
	"strings"
)

// sequence adapts js-sequence's arrows and title to the Mermaid parser.
func sequence(src string) string {
	var body strings.Builder
	var title string
	body.WriteString("sequenceDiagram\n")
	for _, raw := range strings.Split(src, "\n") {
		line := strings.TrimSpace(raw)
		head, value, hasColon := strings.Cut(line, ":")
		switch {
		case strings.EqualFold(head, "title") && hasColon:
			title = strings.TrimSpace(value)
			continue
		case strings.HasPrefix(line, "#"):
			continue
		case hasColon:
			if i := strings.Index(head, "->"); i >= 0 && !strings.HasPrefix(head[i:], "->>") {
				line = head[:i+2] + ">" + head[i+2:] + ":" + value
			}
		}
		body.WriteString(line + "\n")
	}
	if title != "" {
		return "---\ntitle: " + title + "\n---\n" + body.String()
	}
	return body.String()
}

type flowNode struct{ id, shape, label, href string }

var flowShapes = map[string][2]string{
	"start": {"([", "])"}, "end": {"([", "])"},
	"operation": {"[", "]"}, "subroutine": {"[[", "]]"},
	"condition": {"{", "}"}, "inputoutput": {"[/", "/]"},
	"parallel": {"{{", "}}"},
}

// flowchart translates flowchart.js node declarations and path labels. The
// original source, including direction hints and styles, stays in the fence.
func flowchart(src string) (string, error) {
	var nodes []flowNode
	byName := make(map[string]flowNode)
	lines := strings.Split(src, "\n")
	for i, raw := range lines {
		name, definition, ok := strings.Cut(strings.TrimSpace(raw), "=>")
		if !ok || strings.HasPrefix(name, "#") {
			continue
		}
		name = strings.TrimSpace(name)
		shape, label, _ := strings.Cut(definition, ":")
		shape = strings.ToLower(strings.TrimSpace(shape))
		if _, ok := flowShapes[shape]; !ok {
			return "", fmt.Errorf("第 %d 行的节点类型无效 · %s", i+1, shape)
		}
		if _, used := byName[name]; name == "" || used {
			return "", fmt.Errorf("第 %d 行的节点名称为空或重复", i+1)
		}
		label, href, _ := strings.Cut(label, ":>")
		label, _, _ = strings.Cut(label, "|")
		label = strings.TrimSpace(label)
		if label == "" {
			label = name
			if shape == "start" || shape == "end" {
				label = shape
			}
		}
		n := flowNode{id: fmt.Sprintf("n%d", len(nodes)+1), shape: shape, label: label, href: strings.TrimSpace(href)}
		nodes, byName[name] = append(nodes, n), n
	}
	if len(nodes) == 0 {
		return "", fmt.Errorf("没有流程图节点")
	}
	var b strings.Builder
	b.WriteString("flowchart TD\n")
	for _, n := range nodes {
		shape := flowShapes[n.shape]
		// The Mermaid parser uses the unescaped shape punctuation as a delimiter.
		fmt.Fprintf(&b, "%s%s\"%s\"%s\n", n.id, shape[0], flowLabel.Replace(n.label), shape[1])
		if href, _, _ := strings.Cut(n.href, "["); href != "" && !strings.ContainsAny(href, `"<> `) {
			fmt.Fprintf(&b, "click %s href \"%s\"\n", n.id, href)
		}
	}
	for i, raw := range lines {
		line := strings.TrimSpace(raw)
		if line == "" || strings.HasPrefix(line, "#") || strings.Contains(line, "=>") {
			continue
		}
		chain := strings.Split(line, "->")
		if len(chain) < 2 {
			return "", fmt.Errorf("第 %d 行缺少流程连接符 ->", i+1)
		}
		from, label := flowEndpoint(chain[0])
		for _, part := range chain[1:] {
			to, nextLabel := flowEndpoint(part)
			left, l := byName[from]
			right, r := byName[to]
			if !l || !r {
				return "", fmt.Errorf("第 %d 行引用了未定义的节点", i+1)
			}
			fmt.Fprintf(&b, "%s -->", left.id)
			if label != "" {
				fmt.Fprintf(&b, "|%s|", flowLabel.Replace(label))
			}
			fmt.Fprintf(&b, " %s\n", right.id)
			from, label = to, nextLabel
		}
	}
	return b.String(), nil
}

func flowEndpoint(s string) (name, label string) {
	name, arg, _ := strings.Cut(strings.TrimSpace(s), "(")
	label, _, _ = strings.Cut(strings.TrimSuffix(arg, ")"), ",")
	label = strings.TrimSpace(label)
	switch label {
	case "left", "right", "top", "bottom":
		label = ""
	}
	return strings.TrimSpace(name), label
}

var flowLabel = strings.NewReplacer(
	"]", "&#93;", ")", "&#41;", "}", "&#125;", "|", "&#124;",
)

var flowSVGLabel = strings.NewReplacer(
	"&amp;#93;", "&#93;", "&amp;#41;", "&#41;", "&amp;#125;", "&#125;", "&amp;#124;", "&#124;",
)
