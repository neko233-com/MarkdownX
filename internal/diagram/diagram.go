// Package diagram renders the fenced diagram languages used in Markdown.
package diagram

import (
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"strings"

	mermaid "github.com/zkrebbekx/go-mermaid"
)

const (
	maxSource = 64 << 10
	maxLines  = 1024
	maxSVG    = 4 << 20
)

var ErrEmpty = errors.New("请输入图表源码")

// Language returns the diagram language named by a Markdown fence.
func Language(info string) string {
	lang := strings.Fields(info)
	if len(lang) == 0 {
		return ""
	}
	switch name := strings.ToLower(lang[0]); name {
	case "mermaid", "flow", "sequence":
		return name
	default:
		return ""
	}
}

// Render builds static SVG from a Mermaid, flowchart.js or js-sequence fence.
// The original Markdown remains the editable source of the diagram.
func Render(language, source, font string, dark bool) ([]byte, error) {
	if strings.TrimSpace(source) == "" {
		return nil, ErrEmpty
	}
	if len(source) > maxSource || strings.Count(source, "\n") >= maxLines {
		return nil, errors.New("图表源码过大")
	}
	source = strings.ReplaceAll(source, "\r\n", "\n")
	language = Language(language)
	switch language {
	case "flow":
		converted, err := flowchart(source)
		if err != nil {
			return nil, err
		}
		source = converted
	case "sequence":
		source = sequence(source)
	case "mermaid":
	default:
		return nil, errors.New("未知的图表代码块")
	}
	opts := []mermaid.Option{mermaid.WithTransparentBackground()}
	if dark {
		opts = append(opts, mermaid.WithTheme(mermaid.Dark))
	}
	if font == "" {
		font = "sans-serif"
	}
	body, cfg := diagramConfig(source)
	if cfg.FontFamily != "" {
		font = cfg.FontFamily
	}
	size := cfg.FontSize
	if size < 8 || size > 48 {
		size = 14
	}
	opts = append(opts, mermaid.WithFont(font, size))
	switch theme := mermaid.Theme(cfg.Theme); theme {
	case mermaid.Default, mermaid.Dark, mermaid.Neutral, mermaid.Forest, mermaid.Base:
		opts = append(opts, mermaid.WithTheme(theme))
	}
	node, rank := cfg.Flowchart.NodeSpacing, cfg.Flowchart.RankSpacing
	if node < 8 || node > 240 {
		node = 50
	}
	if rank < 8 || rank > 240 {
		rank = 50
	}
	opts = append(opts, mermaid.WithSpacing(node, rank))
	switch cfg.Flowchart.Curve {
	case "basis", "cardinal", "catmullRom", "natural":
		opts = append(opts, mermaid.WithCurvedEdges(true))
	}
	if cfg.Sequence.ShowSequenceNumbers {
		body = numberMessages(body)
	}
	svg, err := mermaid.Render(body, opts...)
	if err != nil {
		if errors.Is(err, mermaid.ErrUnsupported) {
			return nil, errors.New("暂不支持此图表类型")
		}
		var detail *mermaid.ParseError
		if errors.As(err, &detail) {
			return nil, fmt.Errorf("图表语法错误 · %w", detail)
		}
		return nil, fmt.Errorf("无法绘制图表 · %w", err)
	}
	if len(svg) > maxSVG {
		return nil, errors.New("图表渲染结果过大")
	}
	if isFlowchart(body) {
		svg = attachFlowchartEdges(svg)
	}
	if language == "flow" {
		svg = []byte(flowSVGLabel.Replace(string(svg)))
	}
	return svg, nil
}

func isFlowchart(source string) bool {
	front := false
	for raw := range strings.SplitSeq(source, "\n") {
		line := strings.TrimSpace(raw)
		if line == "---" {
			front = !front
			continue
		}
		if front || line == "" || strings.HasPrefix(line, "%%") {
			continue
		}
		head := strings.ToLower(strings.Fields(line)[0])
		return head == "flowchart" || head == "graph"
	}
	return false
}

type config struct {
	Theme      string  `json:"theme"`
	FontFamily string  `json:"fontFamily"`
	FontSize   float64 `json:"fontSize"`
	Flowchart  struct {
		Curve       string  `json:"curve"`
		NodeSpacing float64 `json:"nodeSpacing"`
		RankSpacing float64 `json:"rankSpacing"`
	} `json:"flowchart"`
	Sequence struct {
		ShowSequenceNumbers bool `json:"showSequenceNumbers"`
	} `json:"sequence"`
}

var initDirective = regexp.MustCompile(`(?s)^\s*%%\{\s*init(?:ialize)?\s*:\s*(\{.*?\})\s*\}%%`)
var quotedConfigString = regexp.MustCompile(`"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'`)

func diagramConfig(source string) (string, config) {
	var cfg config
	if match := initDirective.FindStringSubmatchIndex(source); match != nil {
		raw := source[match[2]:match[3]]
		if json.Unmarshal([]byte(raw), &cfg) != nil {
			replace := strings.NewReplacer(`\'`, "'", `"`, `\"`)
			raw = quotedConfigString.ReplaceAllStringFunc(raw, func(s string) string {
				if s[0] != '\'' {
					return s
				}
				return `"` + replace.Replace(s[1:len(s)-1]) + `"`
			})
			cfg = config{}
			if json.Unmarshal([]byte(raw), &cfg) != nil {
				cfg = config{}
			}
		}
		// Keep line numbers below the directive when showing parse errors.
		return strings.Repeat("\n", strings.Count(source[:match[1]], "\n")) + source[match[1]:], cfg
	}
	return source, cfg
}

func numberMessages(source string) string {
	lines := strings.Split(source, "\n")
	for i, line := range lines {
		if strings.EqualFold(strings.TrimSpace(line), "sequenceDiagram") {
			lines[i] += "\nautonumber"
			return strings.Join(lines, "\n")
		}
	}
	return source
}
