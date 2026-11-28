package app

import (
	"bytes"
	"errors"
	"image"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"

	"github.com/neko233/markdownx/internal/convert"
)

const workspacePreviewBytes = 4096
const workspacePreviewImageBytes = 8 << 20

type workspacePreview struct {
	path     string
	modified time.Time
	size     int64
	excerpt  string
	picture  ui.ImageSource
}

func workspaceExcerpt(source string) string {
	more := len(source) > workspacePreviewBytes
	if more {
		end := workspacePreviewBytes
		for end > 0 && !utf8.RuneStart(source[end]) {
			end--
		}
		source = source[:end]
	}
	if !utf8.ValidString(source) || strings.ContainsRune(source, '\x00') {
		return ""
	}
	source = strings.TrimSpace(strings.TrimPrefix(source, utf8BOM))
	lines := strings.SplitN(source, "\n", 9)
	if len(lines) > 8 {
		source, more = strings.Join(lines[:8], "\n"), true
	}
	glyphs := []rune(source)
	if len(glyphs) > 480 {
		source, more = string(glyphs[:480]), true
	}
	source = strings.TrimRight(source, " \r\n\t")
	if more {
		source += "\n\n…"
	}
	return source
}

func workspaceRasterPreview(path string) bool {
	switch strings.ToLower(filepath.Ext(path)) {
	case ".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp":
		return true
	}
	return false
}

func (a *App) workspacePreviewContent(node *workspaceNode, raster bool) workspacePreview {
	if !raster && a.path != "" && sameWorkspacePath(a.path, node.path) {
		return workspacePreview{excerpt: workspaceExcerpt(a.markdownSource())}
	}
	p := &a.treePreview
	if p.path == node.path && p.size == node.info.Size() && p.modified.Equal(node.info.ModTime()) {
		return *p
	}
	*p = workspacePreview{path: node.path, modified: node.info.ModTime(), size: node.info.Size()}
	rel, inside := workspaceRel(a.root, node.path)
	if !inside {
		return *p
	}
	limit := int64(workspacePreviewBytes)
	if raster {
		limit = workspacePreviewImageBytes
		if p.size > limit {
			return *p
		}
	}
	root, err := os.OpenRoot(a.root)
	if err != nil {
		return *p
	}
	defer root.Close()
	f, err := openWorkspaceRegular(root, rel, node.info)
	if err != nil {
		return *p
	}
	defer f.Close()
	data, err := io.ReadAll(io.LimitReader(f, limit+1))
	if err != nil {
		return *p
	}
	if !raster {
		p.excerpt = workspaceExcerpt(string(data))
	} else if len(data) <= workspacePreviewImageBytes {
		cfg, _, err := image.DecodeConfig(bytes.NewReader(data))
		if err == nil && cfg.Width > 0 && cfg.Height > 0 &&
			int64(cfg.Width)*int64(cfg.Height) <= 12<<20 {
			if bitmap, err := ui.DecodeBitmapScaled(data, 320); err == nil {
				p.picture = bitmap
			}
		}
	}
	return *p
}

func (a *App) previewWorkspaceFile(c *ui.Context, row ui.Element, node *workspaceNode) {
	raster := workspaceRasterPreview(node.path)
	if !raster && convert.Detect(node.path) != convert.FormatMarkdown {
		row.Tooltip(node.path)
		return
	}
	ui.TooltipBase(c, row, func(tip ui.Element) {
		if row.Dragging() {
			tip.Opacity(0)
			return
		}
		t := c.Theme()
		tip.AttachTo(row, ui.AnchorEnd, ui.AnchorStart).MarginStart(10).
			Width(344).MaxHeight(306).Padding(10).Background(t.Surface).
			Radius(t.Radius).Border(1, t.Border).Clip()
		tip.Children(func() {
			ui.Column(c).Gap(7).Children(func() {
				rel, _ := workspaceRel(a.root, node.path)
				ui.Text(c, filepath.ToSlash(rel)).FontSize(11).TextColor(t.TextMuted).MaxLines(1)
				preview := a.workspacePreviewContent(node, raster)
				if preview.picture != nil {
					w, h := ui.ImageSize(preview.picture)
					scale := min(float32(1), 322/w, 242/h)
					ui.Image(c, preview.picture).Size(w*scale, h*scale).AlignSelf(ui.Center)
				} else if preview.excerpt != "" {
					markdown.View(c, preview.excerpt, &markdown.Options{
						LoadImage: func(string) (ui.ImageSource, error) { return nil, errors.New("文件预览只读取正文") },
					}).MaxHeight(260)
				} else {
					ui.Text(c, "没有可预览的内容").FontSize(12).TextColor(t.TextMuted)
				}
			})
		})
	})
}
