package app

import (
	"encoding/base64"
	"errors"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/egoist/mygo/ui/markdown/parser"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/neko233/markdownx/internal/doc"
)

const maxImageBytes = 20 << 20

func imageReference(document, path string) (string, error) {
	if document == "" {
		return filepath.ToSlash(path), nil
	}
	dir := filepath.Dir(document)
	if rel, err := filepath.Rel(dir, path); err == nil && filepath.IsLocal(rel) {
		return filepath.ToSlash(rel), nil
	}
	assets := filepath.Join(dir, "assets")
	if err := os.MkdirAll(assets, 0o755); err != nil {
		return "", err
	}
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	data, err := io.ReadAll(io.LimitReader(f, maxImageBytes+1))
	if err != nil || len(data) > maxImageBytes {
		return "", errors.New("图片超过 20 MB 或无法读取")
	}
	ext := filepath.Ext(path)
	stem := strings.TrimSuffix(filepath.Base(path), ext)
	for n := 1; n <= 1000; n++ {
		name := stem + ext
		if n > 1 {
			name = stem + "-" + itoa(n) + ext
		}
		dest := filepath.Join(assets, name)
		out, err := os.OpenFile(dest, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
		if errors.Is(err, os.ErrExist) {
			continue
		}
		if err != nil {
			return "", err
		}
		_, writeErr := out.Write(data)
		if err := errors.Join(writeErr, out.Close()); err != nil {
			_ = os.Remove(dest)
			return "", err
		}
		return "assets/" + name, nil
	}
	return "", errors.New("图片同名文件过多")
}

func (a *App) resetPreview() {
	tab := a.documentTab
	dir := tab.imageDirectory(a.root)
	tab.previewFolder = dir
	loadImage := func(ref string) (ui.ImageSource, error) {
		return loadDocumentImage(dir, ref)
	}
	a.md = markdown.Options{
		Extensions: parser.Default | parser.ExtDollarMath,
		Math:       a.previewMath,
		InlineMath: a.previewInlineMath,
		CodeBlock:  a.previewCode,
		Block:      a.previewContents,
		OnLink: func(target string) {
			if a.activatePaneInput(tab) {
				tab.pendingLink = target
				a.services.Invalidate()
			}
		},
		LoadImage: loadImage,
	}
	a.imageMD = markdown.Options{LoadImage: loadImage, ShareSelection: true}
}

func (p *documentTab) imageDirectory(root string) string {
	if p.path != "" {
		return filepath.Dir(p.path)
	}
	return root
}

func (p *documentTab) ensurePreview(a *App) {
	if p.md.LoadImage == nil || p.previewFolder != p.imageDirectory(a.root) {
		a.resetPreview()
	}
}

func loadDocumentImage(dir, ref string) (ui.ImageSource, error) {
	raw, svg, err := documentImageBytes(dir, ref)
	if err != nil {
		return nil, err
	}
	if svg {
		return ui.ParseSVG(raw)
	}
	return ui.DecodeBitmap(raw)
}

func documentImageBytes(dir, ref string) ([]byte, bool, error) {
	if strings.HasPrefix(ref, "data:image/") {
		header, payload, ok := strings.Cut(ref, ",")
		if !ok || !strings.HasSuffix(header, ";base64") || len(payload) > base64.StdEncoding.EncodedLen(maxImageBytes) {
			return nil, false, errors.New("invalid image data URL")
		}
		raw, err := base64.StdEncoding.DecodeString(payload)
		if err != nil || len(raw) > maxImageBytes {
			return nil, false, errors.New("invalid image data")
		}
		return raw, strings.HasPrefix(header, "data:image/svg+xml;"), nil
	}
	path, absolute := nativeFileURLPath(ref)
	if !absolute {
		decoded, err := url.PathUnescape(ref)
		if err != nil {
			return nil, false, err
		}
		path = filepath.FromSlash(decoded)
	}
	if strings.HasPrefix(filepath.VolumeName(path), `\\`) {
		return nil, false, os.ErrNotExist
	}
	if !filepath.IsAbs(path) {
		if dir == "" || strings.Contains(path, ":") {
			return nil, false, os.ErrNotExist
		}
		path = filepath.Join(dir, path)
	}
	info, err := os.Stat(path)
	if err != nil || !info.Mode().IsRegular() || info.Size() > maxImageBytes {
		return nil, false, os.ErrNotExist
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, false, err
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, maxImageBytes+1))
	if err != nil || len(raw) > maxImageBytes {
		return nil, false, errors.New("image is too large or unreadable")
	}
	return raw, strings.EqualFold(filepath.Ext(path), ".svg"), nil
}

func (a *App) inlineAttachment(in *doc.Inline) *richtext.Attachment {
	if ref, ok := in.Reference(); ok {
		return a.referenceAttachment(in, ref)
	}
	att := &richtext.Attachment{Data: in}
	if formula, ok := in.Equation(); ok {
		att.Name = formula
		display := in.IsDisplayEquation()
		size := float32(15)
		if display {
			size = 18
		}
		img := a.mathImage(formula, size, display, a.inlineColor)
		if img.source != nil {
			att.Image = img.source
			att.Width, att.Height = img.width, img.ascent+img.descent
			return att
		}
	} else if img, ok := in.Image(); ok {
		att.Name, att.URL = img.Alt, img.Src
		att.Image, _ = a.md.LoadImage(img.Src)
	} else if file, ok := in.File(); ok {
		att.Name = file.Name
	}
	if att.Image != nil {
		w, h := ui.ImageSize(att.Image)
		scale := min(float32(1), 180/max(w, 1), 76/max(h, 1))
		att.Width, att.Height = w*scale, h*scale
		return att
	}
	if att.Name == "" {
		att.Name = "附件"
	}
	label := strings.Join(strings.Fields(att.Name), " ")
	if n := utf8.RuneCountInString(label); n > 48 {
		label = label[:doc.RuneToByte(label, 47)] + "…"
	}
	att.Width, att.Height = max(16, float32(utf8.RuneCountInString(label))*9), 19
	att.Draw = func(p *ui.Painter, r ui.Rect) {
		p.Text(r.X, r.Y, label, 13, a.inlineColor)
	}
	return att
}
