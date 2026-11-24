// Package book reads and writes `.mdxb` books: one JSON file holding a
// bilingual, multi-chapter document whose chapters are DocxXML or Markdown and
// whose media travel base64-encoded inside it.
package book

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"path"
	"strings"

	"github.com/neko233/markdownx/internal/doc"
)

// Format is the marker every book carries.
const Format = "markdownx.book"

// Version is the newest supported book version.
const Version = 2

// Book is the file's envelope.
type Book struct {
	Format   string    `json:"format"`
	Version  int       `json:"version"`
	ID       string    `json:"id,omitempty"`
	Title    Localized `json:"title"`
	Subtitle Localized `json:"subtitle,omitempty"`
	Authors  []string  `json:"authors,omitempty"`
	// Languages are the editions the book carries, Chinese first.
	Languages       []string `json:"languages,omitempty"`
	DefaultLanguage string   `json:"default_language,omitempty"`
	Cover           string   `json:"cover,omitempty"`
	Theme           Theme    `json:"theme,omitempty"`
	Reading         Reading  `json:"reading,omitempty"`
	Chapters        []Chapter `json:"chapters"`
	// Assets holds every media file the chapters reference, base64-encoded.
	Assets    map[string]Asset `json:"assets,omitempty"`
	Created   int64            `json:"created,omitempty"`
	Modified  int64            `json:"modified,omitempty"`
	Generator string           `json:"generator,omitempty"`
}

// Localized is one string in the book's languages.
type Localized struct {
	ZH string `json:"zh,omitempty"`
	EN string `json:"en,omitempty"`
}

// Text is the Chinese edition, else the English one.
func (l Localized) Text() string {
	if s := strings.TrimSpace(l.ZH); s != "" {
		return s
	}
	return strings.TrimSpace(l.EN)
}

// UnmarshalJSON accepts a plain string as well as the object form.
func (l *Localized) UnmarshalJSON(data []byte) error {
	var s string
	if err := json.Unmarshal(data, &s); err == nil {
		l.ZH = s
		return nil
	}
	type alias Localized
	var a alias
	if err := json.Unmarshal(data, &a); err != nil {
		return err
	}
	*l = Localized(a)
	return nil
}

// Theme is the book's look.
type Theme struct {
	Accent     string `json:"accent,omitempty"`
	Background string `json:"background,omitempty"`
	Text       string `json:"text,omitempty"`
	Font       string `json:"font,omitempty"`
}

// Reading is how the reader shows the book.
type Reading struct {
	Layout   string  `json:"layout,omitempty"`
	MaxWidth float32 `json:"max_width,omitempty"`
	ShowTOC  bool    `json:"show_toc,omitempty"`
}

// Chapter is one chapter, in one or two languages.
type Chapter struct {
	ID            string    `json:"id"`
	Title         Localized `json:"title,omitempty"`
	Format        string    `json:"format,omitempty"`
	Content       string    `json:"content"`
	FormatEN      string    `json:"format_en,omitempty"`
	ContentEN     string    `json:"content_en,omitempty"`
	SourcePath    string    `json:"source_path,omitempty"`
	SourceENPath  string    `json:"source_en_path,omitempty"`
}

// Asset is one embedded file.
type Asset struct {
	MIME string `json:"mime,omitempty"`
	Data string `json:"data"`
}

// ErrInvalid is wrapped by every parse error.
var ErrInvalid = fmt.Errorf("不是有效的 .mdxb 文件")

// Parse reads a book from its file bytes.
func Parse(data []byte) (*Book, error) {
	var b Book
	if err := json.Unmarshal(data, &b); err != nil {
		return nil, fmt.Errorf("%w: %v", ErrInvalid, err)
	}
	if b.Format != Format {
		return nil, fmt.Errorf("%w: 缺少 format 标记", ErrInvalid)
	}
	if b.Version < 1 || b.Version > Version {
		return nil, fmt.Errorf("%w: 不支持的版本 %d", ErrInvalid, b.Version)
	}
	if len(b.Chapters) == 0 {
		return nil, fmt.Errorf("%w: 书里没有章节", ErrInvalid)
	}
	for i := range b.Chapters {
		if b.Chapters[i].ID == "" {
			b.Chapters[i].ID = fmt.Sprintf("chapter-%d", i+1)
		}
		if b.Chapters[i].Format == "" {
			b.Chapters[i].Format = "docxxml"
		}
	}
	if len(b.Languages) == 0 {
		b.Languages = []string{"zh", "en"}
	}
	if b.DefaultLanguage == "" {
		b.DefaultLanguage = b.Languages[0]
	}
	if b.Reading.Layout == "" {
		b.Reading.Layout = "scroll"
	}
	if b.Reading.MaxWidth == 0 {
		b.Reading.MaxWidth = 760
	}
	return &b, nil
}

// Bytes renders the book as the file's bytes.
func (b *Book) Bytes() ([]byte, error) {
	out := *b
	if out.Format == "" {
		out.Format = Format
	}
	if out.Version == 0 {
		out.Version = Version
	}
	if out.Generator == "" {
		out.Generator = "MarkdownX"
	}
	data, err := json.MarshalIndent(out, "", "  ")
	if err != nil {
		return nil, err
	}
	return append(data, '\n'), nil
}

// Blocks returns a chapter's content as document blocks.
func (c *Chapter) Blocks() ([]*doc.Block, error) {
	switch strings.ToLower(c.Format) {
	case "", "docxxml", "docxml":
		return ParseDocxXML(c.Content), nil
	case "markdown", "md":
		return doc.MarkdownToBlocks(c.Content, doc.Off{File: true}), nil
	}
	return nil, fmt.Errorf("%w: 未知章节格式 %q", ErrInvalid, c.Format)
}

// BlocksEN returns the translated chapter, or nil when there is none.
func (c *Chapter) BlocksEN() ([]*doc.Block, error) {
	if strings.TrimSpace(c.ContentEN) == "" {
		return nil, nil
	}
	kind := c.FormatEN
	if kind == "" {
		kind = c.Format
	}
	sub := *c
	sub.Format = kind
	sub.Content = c.ContentEN
	return sub.Blocks()
}

// Document flattens the whole book into one document, one heading per
// chapter, so the desktop app can read and edit it.
func (b *Book) Document() *doc.Document {
	d := &doc.Document{Title: doc.PlainRich(b.Title.Text())}
	for _, ch := range b.Chapters {
		blocks, err := ch.Blocks()
		if err != nil {
			continue
		}
		level := uint8(1)
		title := strings.TrimSpace(ch.Title.Text())
		if title != "" {
			d.Blocks = append(d.Blocks, doc.NewBlock(doc.HeadingKind{Level: level}, doc.PlainRich(title)))
		}
		d.Blocks = append(d.Blocks, blocks...)
	}
	if len(d.Blocks) == 0 {
		d.Blocks = []*doc.Block{doc.TextBlock("")}
	}
	return d
}

// UpdateFromDocument rewrites a book's chapters from an edited document while
// keeping its envelope: identity, authors, theme, reading settings and assets.
//
// A top-level level-1 heading starts a chapter, so a book that was opened as
// `# chapter` headings saves back as the same chapters. A chapter whose title
// matches an existing one keeps its id, format and translation.
func (b *Book) UpdateFromDocument(d *doc.Document) {
	type chapterDraft struct {
		title  string
		blocks []*doc.Block
	}
	var drafts []chapterDraft
	cur := chapterDraft{title: b.Title.Text()}
	for _, blk := range d.Blocks {
		if h, ok := blk.Kind.(doc.HeadingKind); ok && h.Level == 1 {
			if cur.title != "" || len(cur.blocks) > 0 {
				drafts = append(drafts, cur)
			}
			cur = chapterDraft{title: blk.Text.PlainText()}
			continue
		}
		cur.blocks = append(cur.blocks, blk.Clone())
	}
	drafts = append(drafts, cur)
	for len(drafts) > 1 && drafts[0].title == b.Title.Text() && len(drafts[0].blocks) == 0 {
		drafts = drafts[1:]
	}

	byTitle := map[string]*Chapter{}
	for i := range b.Chapters {
		byTitle[b.Chapters[i].Title.Text()] = &b.Chapters[i]
	}
	out := make([]Chapter, 0, len(drafts))
	for i, dr := range drafts {
		ch := Chapter{
			ID:      fmt.Sprintf("chapter-%d", i+1),
			Title:   Localized{ZH: dr.title},
			Format:  "docxxml",
			Content: ToDocxXML(dr.blocks),
		}
		if old, ok := byTitle[dr.title]; ok {
			ch.ID = old.ID
			ch.Format = old.Format
			ch.SourcePath = old.SourcePath
			ch.SourceENPath = old.SourceENPath
			ch.ContentEN = old.ContentEN
			ch.FormatEN = old.FormatEN
			ch.Title = old.Title
		}
		if strings.TrimSpace(dr.title) == "" {
			ch.Title = Localized{ZH: fmt.Sprintf("第 %d 章", i+1)}
		}
		out = append(out, ch)
	}
	b.Chapters = out
	if b.Format == "" {
		b.Format = Format
	}
	if b.Version == 0 {
		b.Version = Version
	}
}

// FromDocument wraps one document as a single-chapter book.
func FromDocument(d *doc.Document, title string) *Book {
	blocks := make([]*doc.Block, 0, len(d.Blocks))
	for _, b := range d.Blocks {
		blocks = append(blocks, b.Clone())
	}
	return &Book{
		Format:   Format,
		Version:  Version,
		Title:    Localized{ZH: title},
		Chapters: []Chapter{{ID: "chapter-1", Title: Localized{ZH: title}, Format: "docxxml", Content: ToDocxXML(blocks)}},
		Assets:   map[string]Asset{},
	}
}

// AssetBytes decodes one embedded file.
func (b *Book) AssetBytes(name string) ([]byte, string, bool) {
	a, ok := b.Assets[path.Clean(strings.TrimPrefix(name, "./"))]
	if !ok {
		return nil, "", false
	}
	data, err := base64.StdEncoding.DecodeString(a.Data)
	if err != nil {
		return nil, "", false
	}
	mime := a.MIME
	if mime == "" {
		mime = GuessMIME(name)
	}
	return data, mime, true
}

// GuessMIME names the media type of a file by its extension.
func GuessMIME(name string) string {
	ext := strings.ToLower(path.Ext(name))
	switch ext {
	case ".png":
		return "image/png"
	case ".jpg", ".jpeg":
		return "image/jpeg"
	case ".gif":
		return "image/gif"
	case ".webp":
		return "image/webp"
	case ".svg":
		return "image/svg+xml"
	case ".pdf":
		return "application/pdf"
	case ".mp4", ".m4v":
		return "video/mp4"
	case ".mov":
		return "video/quicktime"
	case ".webm":
		return "video/webm"
	case ".mp3":
		return "audio/mpeg"
	case ".wav":
		return "audio/wav"
	case ".ogg", ".opus":
		return "audio/ogg"
	case ".vtt":
		return "text/vtt"
	case ".js", ".mjs":
		return "text/javascript"
	case ".css":
		return "text/css"
	}
	return "application/octet-stream"
}
