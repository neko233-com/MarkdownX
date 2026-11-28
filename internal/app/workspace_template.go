package app

import (
	"crypto/rand"
	"fmt"
	"iter"
	"path/filepath"
	"strconv"
	"strings"
	"time"
	"unicode"

	"go.yaml.in/yaml/v3"
)

type workspaceTemplate struct {
	template
	filename, description string
	frontMatter           *templateFrontMatter
}

type templateValues struct {
	now           time.Time
	title         string
	uuid          string
	readClipboard func() string
	clipboard     string
}

type templateFrontMatter struct {
	start, end, after int
	root              *yaml.Node
	metadata          int
}

type workspaceTemplateField struct {
	number, start, end int
}

type workspaceTemplateOutput struct {
	text        string
	cursor      int
	cursorAfter int
	fields      []workspaceTemplateField
}

func prepareWorkspaceTemplate(t template) workspaceTemplate {
	chosen := workspaceTemplate{template: t, frontMatter: readTemplateFrontMatter(t.Body)}
	if f := chosen.frontMatter; f != nil && f.metadata >= 0 {
		metadata := f.root.Content[f.metadata+1]
		chosen.filename = templateYAMLValue(metadata, "filename")
		chosen.description = templateYAMLValue(metadata, "description")
	}
	return chosen
}

func templateYAMLValue(n *yaml.Node, key string) string {
	if n.Kind == yaml.AliasNode {
		n = n.Alias
	}
	if n == nil || n.Kind != yaml.MappingNode {
		return ""
	}
	for i := 0; i < len(n.Content); i += 2 {
		if n.Content[i].Value != key {
			continue
		}
		v := n.Content[i+1]
		if v.Kind == yaml.AliasNode {
			v = v.Alias
		}
		if v != nil && v.Kind == yaml.ScalarNode && v.Tag != "!!null" {
			return v.Value
		}
	}
	return ""
}

func readTemplateFrontMatter(body string) *templateFrontMatter {
	text := strings.TrimPrefix(body, utf8BOM)
	first, next := templateLine(text)
	if first != "---" || next == len(text) {
		return nil
	}
	start := len(body) - len(text) + next
	for end := start; end < len(body); {
		line, width := templateLine(body[end:])
		if line != "---" && line != "..." {
			end += width
			continue
		}
		var file yaml.Node
		if yaml.Unmarshal([]byte(body[start:end]), &file) != nil || len(file.Content) == 0 || file.Content[0].Kind != yaml.MappingNode {
			return nil
		}
		root, metadata := file.Content[0], -1
		for i := 0; i < len(root.Content); i += 2 {
			if k := root.Content[i]; k.Kind == yaml.ScalarNode && k.Value == "typora-template" {
				metadata = i
				break
			}
		}
		return &templateFrontMatter{start: start, end: end, after: end + width, root: root, metadata: metadata}
	}
	return nil
}

func templateLine(text string) (line string, width int) {
	i := strings.IndexAny(text, "\r\n")
	if i < 0 {
		return text, len(text)
	}
	width = i + 1
	if strings.HasPrefix(text[i:], "\r\n") {
		width++
	}
	return text[:i], width
}

func templateLineEnding(text string) string {
	line, width := templateLine(text)
	if width > len(line) {
		return text[len(line):width]
	}
	return "\n"
}

func (t workspaceTemplate) suggestedName(v *templateValues) string {
	v.title = t.Name
	name := expandWorkspaceTemplate(t.filename, v, "\n", false).text
	name = safeTemplateName(strings.Map(func(r rune) rune {
		if unicode.IsControl(r) {
			return ' '
		}
		return r
	}, firstNonEmpty(name, t.Name)))
	if _, err := workspaceEntryName(name); err != nil {
		return "未命名文档"
	}
	return name
}

func (t workspaceTemplate) render(name string, v *templateValues) (workspaceTemplateOutput, error) {
	v.title = strings.TrimSuffix(name, filepath.Ext(name))
	body, prefix := t.Body, ""
	eol := templateLineEnding(body)
	if f := t.frontMatter; f != nil {
		var err error
		prefix, err = f.render(body, v, eol)
		if err != nil {
			return workspaceTemplateOutput{}, err
		}
		body = body[f.after:]
	}
	out := expandWorkspaceTemplate(body, v, eol, true)
	if out.cursor >= 0 {
		out.cursor += len(prefix)
	}
	for i := range out.fields {
		out.fields[i].start += len(prefix)
		out.fields[i].end += len(prefix)
	}
	out.text = prefix + out.text
	return out, nil
}

func (f *templateFrontMatter) render(body string, v *templateValues, eol string) (string, error) {
	root := *f.root
	if f.metadata >= 0 {
		root.Content = append([]*yaml.Node(nil), root.Content[:f.metadata]...)
		root.Content = append(root.Content, f.root.Content[f.metadata+2:]...)
		if len(root.Content) == 0 {
			if strings.HasPrefix(body, utf8BOM) {
				return utf8BOM, nil
			}
			return "", nil
		}
	}
	updated := expandWorkspaceYAML(&root, v)
	if updated == &root && f.metadata < 0 {
		return body[:f.after], nil
	}
	data := body[f.start:f.end]
	var spliced bool
	if updated == &root && root.Style&yaml.FlowStyle == 0 {
		data, spliced = f.withoutMetadata(data)
	}
	if !spliced {
		encoded, err := yaml.Marshal(updated)
		if err != nil {
			return "", fmt.Errorf("无法展开模板元数据（%w）", err)
		}
		data = strings.ReplaceAll(string(encoded), "\n", eol)
	}
	var parsed yaml.Node
	if err := yaml.Unmarshal([]byte(data), &parsed); err != nil {
		return "", fmt.Errorf("模板展开后的 YAML 无效（%w）", err)
	}
	return body[:f.start] + data + body[f.end:f.after], nil
}

// Remove the reserved key by line so unrelated YAML spelling stays intact.
func (f *templateFrontMatter) withoutMetadata(data string) (string, bool) {
	key := f.root.Content[f.metadata]
	var starts []int
	for offset := 0; offset < len(data); {
		starts = append(starts, offset)
		_, width := templateLine(data[offset:])
		offset += width
	}
	starts = append(starts, len(data))
	last := len(starts) - 1
	if f.metadata+2 < len(f.root.Content) {
		last = f.root.Content[f.metadata+2].Line - 1
	}
	if key.Line < 1 || key.Line > last || last >= len(starts) {
		return "", false
	}
	for last > key.Line {
		line, _ := templateLine(data[starts[last-1]:])
		trimmed := strings.TrimLeft(line, " ")
		if strings.TrimSpace(line) != "" && (!strings.HasPrefix(trimmed, "#") || len(line)-len(trimmed) >= key.Column) {
			break
		}
		last--
	}
	return data[:starts[key.Line-1]] + data[starts[last]:], true
}

// Copy only the YAML nodes changed by interpolation. Encode string values as
// YAML scalars so interpolated quotes, hashes and line breaks remain data.
func expandWorkspaceYAML(node *yaml.Node, v *templateValues) *yaml.Node {
	if node.Kind == yaml.ScalarNode && node.Tag == "!!str" {
		value := expandWorkspaceTemplate(node.Value, v, "\n", false).text
		if value != node.Value {
			next := *node
			next.Value = value
			return &next
		}
	}
	var copyNode *yaml.Node
	for i, child := range node.Content {
		if next := expandWorkspaceYAML(child, v); next != child {
			if copyNode == nil {
				clone := *node
				clone.Content = append([]*yaml.Node(nil), node.Content...)
				copyNode = &clone
			}
			copyNode.Content[i] = next
		}
	}
	if copyNode != nil {
		return copyNode
	}
	return node
}

func workspaceTemplateChunks(text string) iter.Seq2[string, bool] {
	return func(yield func(string, bool) bool) {
		source := text
		for {
			start := strings.Index(source, "{{")
			if start < 0 {
				yield(source, false)
				return
			}
			if start > 0 && source[start-1] == '\\' {
				if !yield(source[:start-1], false) || !yield("{{", false) {
					return
				}
				source = source[start+2:]
				continue
			}
			end := strings.Index(source[start+2:], "}}")
			if end < 0 {
				yield(source, false)
				return
			}
			end += start + 4
			if !yield(source[:start], false) || !yield(source[start:end], true) {
				return
			}
			source = source[end:]
		}
	}
}

func workspaceTemplateNumber(rule string) (int, string, bool) {
	key, fallback, declared := strings.Cut(rule, ":")
	key = strings.TrimSpace(key)
	for _, r := range key {
		if r < '0' || r > '9' {
			return 0, "", false
		}
	}
	number, err := strconv.Atoi(key)
	if err != nil || number <= 0 {
		return 0, "", false
	}
	return number, fallback, declared
}

func expandWorkspaceTemplate(source string, v *templateValues, eol string, document bool) workspaceTemplateOutput {
	result := workspaceTemplateOutput{text: source, cursor: -1}
	if !strings.Contains(source, "{{") {
		return result
	}
	chunks := workspaceTemplateChunks(source)
	defaults := map[int]string{}
	if document {
		for chunk, variable := range chunks {
			if variable {
				if number, value, declared := workspaceTemplateNumber(chunk[2 : len(chunk)-2]); declared {
					if _, exists := defaults[number]; !exists {
						defaults[number] = value
					}
				}
			}
		}
	}
	var out strings.Builder
	for chunk, variable := range chunks {
		if !variable {
			out.WriteString(chunk)
			continue
		}
		raw := chunk[2 : len(chunk)-2]
		rule := strings.TrimSpace(raw)
		if document {
			if rule == "cursor" {
				if result.cursor < 0 {
					result.cursor = out.Len()
					result.cursorAfter = len(result.fields)
				}
				continue
			}
			if number, _, _ := workspaceTemplateNumber(raw); number > 0 {
				start := out.Len()
				out.WriteString(defaults[number])
				result.fields = append(result.fields, workspaceTemplateField{number, start, out.Len()})
				continue
			}
		}
		if value, ok := v.value(rule); ok {
			out.WriteString(strings.ReplaceAll(value, "\n", eol))
		} else {
			out.WriteString(chunk)
		}
	}
	result.text = out.String()
	return result
}

func (v *templateValues) value(rule string) (string, bool) {
	key, format, formatted := strings.Cut(rule, ":")
	switch key {
	case "date", "time":
		if formatted {
			return workspaceMoment(v.now, format), true
		}
		if key == "date" {
			return v.now.Format(time.DateOnly), true
		}
		return v.now.Format("15:04"), true
	}
	if formatted {
		return "", false
	}
	switch key {
	case "title":
		return v.title, true
	case "clipboard":
		if v.readClipboard != nil {
			v.clipboard = strings.NewReplacer("\r\n", "\n", "\r", "\n").Replace(v.readClipboard())
			v.readClipboard = nil
		}
		return v.clipboard, true
	case "uuid":
		if v.uuid == "" {
			var id [16]byte
			rand.Read(id[:])
			id[6] = id[6]&0x0f | 0x40
			id[8] = id[8]&0x3f | 0x80
			v.uuid = fmt.Sprintf("%x-%x-%x-%x-%x", id[:4], id[4:6], id[6:8], id[8:10], id[10:])
		}
		return v.uuid, true
	}
	return "", false
}

var workspaceMomentLayouts = map[string]string{
	"YYYY": "2006", "YY": "06", "MMMM": "January", "MMM": "Jan", "MM": "01", "M": "1",
	"DD": "02", "D": "2", "dddd": "Monday", "ddd": "Mon", "HH": "15", "hh": "03", "h": "3",
	"mm": "04", "m": "4", "ss": "05", "s": "5", "A": "PM", "a": "pm",
}

func workspaceMoment(now time.Time, format string) string {
	var out strings.Builder
	for len(format) > 0 {
		if format[0] == '[' {
			if end := strings.IndexByte(format, ']'); end >= 0 {
				out.WriteString(format[1:end])
				format = format[end+1:]
				continue
			}
		}
		width := min(4, len(format))
		for ; width > 0; width-- {
			token := format[:width]
			if token == "H" {
				out.WriteString(strconv.Itoa(now.Hour()))
				break
			}
			if layout, ok := workspaceMomentLayouts[token]; ok {
				out.WriteString(now.Format(layout))
				break
			}
		}
		if width == 0 {
			out.WriteByte(format[0])
			width = 1
		}
		format = format[width:]
	}
	return out.String()
}
