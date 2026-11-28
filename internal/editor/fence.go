package editor

import (
	"strings"

	"github.com/neko233/markdownx/internal/doc"
)

// SetCodeLanguage changes a fence's info string without touching its content.
func (c *Core) SetCodeLanguage(id doc.BlockID, lang string) bool {
	b := c.Block(id)
	if b == nil {
		return false
	}
	code, ok := b.Kind.(doc.CodeKind)
	lang = strings.TrimSpace(lang)
	if !ok || code.FrontMatter || lang == code.Lang || strings.ContainsAny(lang, "`\r\n\x00") {
		return false
	}
	c.Checkpoint(EditOther)
	code.Lang = lang
	b.Kind = code
	c.touch()
	return true
}
