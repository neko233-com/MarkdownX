package app

import (
	"strings"

	"github.com/egoist/mygo/ui"
	syntax "github.com/egoist/mygo/ui/editor"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type fenceLanguage struct {
	from, draft string
	focused     bool
}

func (a *App) codeEditor(c *ui.Context, b *doc.Block) {
	t := c.Theme()
	ui.Column(c).Padding(10, 12).Gap(4).Radius(6).Background(t.Surface).Children(func() {
		a.blockInput(c, b, 13)
		code := b.Kind.(doc.CodeKind)
		if code.FrontMatter {
			return
		}
		p := a.bufs[b.ID]
		lang := &p.language
		info := code.Lang
		if lang.from != info {
			lang.from, lang.draft = info, info
		}
		field := ui.TextInputBase(c.Key("language"), &lang.draft).AlignSelf(ui.End).
			Width(140).Padding(3, 6).FontSize(11).TextColor(t.TextMuted).Placeholder("语言").Label("代码块语言")
		field.OnTextKey(func(k ui.TextKey) bool {
			if k.Mods != 0 || (k.Key != ui.KeyEnter && k.Key != ui.KeyEscape) {
				return false
			}
			if k.Key == ui.KeyEnter {
				a.setFenceLanguage(b, k.Text())
			}
			lang.from = b.Kind.(doc.CodeKind).Lang
			lang.draft = lang.from
			a.focusAt(editor.Caret(editor.Pos{Block: b.ID, Offset: a.sel[b.ID][1]}))
			return true
		})
		focused := field.Focused()
		if lang.focused && !focused {
			a.setFenceLanguage(b, lang.draft)
			lang.from = b.Kind.(doc.CodeKind).Lang
			lang.draft = lang.from
		}
		lang.focused = focused
	})
}

func (a *App) setFenceLanguage(b *doc.Block, language string) {
	if language == b.Kind.(doc.CodeKind).Lang {
		return
	}
	a.syncAll()
	if a.core.SetCodeLanguage(b.ID, language) {
		a.ResetBuffers()
		a.services.Invalidate()
	}
}

type codeSyntax struct {
	info, text string
	dark       bool
	version    int
	lines      []codeSyntaxLine
}

type codeSyntaxLine struct {
	text  string
	spans []ui.Span
}

// Color the native layout without adding formatting to the stored code.
func (s *codeSyntax) prepare(b *doc.Block, theme *ui.Theme) {
	k, code := b.Kind.(doc.CodeKind)
	if !code {
		if s.info != "" {
			*s = codeSyntax{version: s.version + 1}
		}
		return
	}
	if s.info == k.Lang && s.text == b.Text.Text && s.dark == theme.Dark {
		return
	}
	s.info, s.text, s.dark, s.lines = k.Lang, b.Text.Text, theme.Dark, nil
	s.version++
	lang := syntax.LanguageNamed(k.Lang)
	if lang == nil {
		if name, _, ok := strings.Cut(k.Lang, " "); ok {
			lang = syntax.LanguageNamed(name)
		}
	}
	if lang == nil || lang.Tokenizer == nil {
		return
	}
	style := syntax.DefaultTheme(theme)
	var state uint64
	var tokens []syntax.Token
	for line := range strings.SplitSeq(s.text, "\n") {
		tokens, state = lang.Tokenizer.Tokenize(line, state, tokens[:0])
		spans := make([]ui.Span, 0, len(tokens)+1)
		start := 0
		for _, token := range tokens {
			end := min(token.End, len(line))
			if end <= start {
				continue
			}
			look := style.Tokens[token.Kind]
			span := ui.Span{Text: line[start:end], Color: look.Color, Italic: look.Italic}
			if look.Bold {
				span.Weight = 700
			}
			spans = append(spans, span)
			start = end
		}
		if start < len(line) {
			spans = append(spans, ui.Span{Text: line[start:]})
		}
		s.lines = append(s.lines, codeSyntaxLine{text: line, spans: spans})
	}
}

func (s *codeSyntax) paint(line int, spans []ui.Span) []ui.Span {
	// A composed IME string can differ from the committed paragraph.
	if line < len(s.lines) && len(spans) == 1 && spans[0].Text == s.lines[line].text && len(s.lines[line].spans) != 0 {
		return s.lines[line].spans
	}
	return spans
}
