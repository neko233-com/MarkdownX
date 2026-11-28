package app

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type workspaceTemplateSlot struct {
	workspaceTemplateField
	block doc.BlockID
}

type workspaceTemplateSession struct {
	slots         []workspaceTemplateSlot
	numbers       []int
	group, active int
	finish        editor.Pos
	cursor, tail  int
	cursorAfter   int
	generation    uint64
	source        *sourceView
	text          string
	outsideClick  bool
	sourceClick   bool
	mergedSource  bool
}

func (a *App) startWorkspaceTemplateSession(out workspaceTemplateOutput) {
	visual, ranges := a.visualSource()
	text := a.file.text
	cursor := out.cursor
	beforeCursor := out.cursorAfter
	if cursor < 0 {
		cursor = len(out.text)
		beforeCursor = len(out.fields)
	}
	cursor = templateLFOffset(out.text, cursor)
	s := &workspaceTemplateSession{cursor: cursor, tail: len(text) - cursor, cursorAfter: beforeCursor}
	for _, field := range out.fields {
		field.start = templateLFOffset(out.text, field.start)
		field.end = templateLFOffset(out.text, field.end)
		s.slots = append(s.slots, workspaceTemplateSlot{workspaceTemplateField: field})
		s.numbers = append(s.numbers, field.number)
	}
	slices.Sort(s.numbers)
	s.numbers = slices.Compact(s.numbers)
	if visual != text || !a.mapWorkspaceTemplateSlots(s, text, ranges) {
		a.enterSourceAt(text, 0)
		s.source, s.text = a.source, a.source.text()
	} else if s.finish.Block == 0 {
		// Prepare an empty final paragraph before the first field edit so
		// leaving the last field doesn't add a step to the undo history.
		s.finish, _ = a.workspaceTemplateVisualCursor(text, ranges, cursor)
	}
	s.generation = a.core.Generation
	a.treeFocus, a.treeTarget = false, ""
	a.templateSession = s
	a.selectWorkspaceTemplateField()
}

func (a *App) mapWorkspaceTemplateSlots(s *workspaceTemplateSession, text string, ranges []doc.SourceRange) bool {
	if a.mode == ModePreview {
		return false
	}
	finish, found := a.workspaceTemplateMappedCaret(text, ranges, s.cursor)
	if !found && len(ranges) > 0 && (s.cursor < ranges[len(ranges)-1].End ||
		strings.TrimSpace(text[ranges[len(ranges)-1].End:]) != "") {
		return false
	}
	slots := slices.Clone(s.slots)
	for i, slot := range slots {
		var start, end editor.Pos
		if slot.start == slot.end {
			p, ok := a.workspaceTemplateMappedCaret(text, ranges, slot.start)
			if !ok {
				return false
			}
			start, end = p, p
		} else {
			sel, ok := a.workspaceMatchSelection(text, slot.start, slot.end)
			if !ok {
				return false
			}
			start, end = sel.Anchor, sel.Head
		}
		if start.Block == 0 || start.Block != end.Block || (i > 0 && slots[i-1].block == start.Block && slots[i-1].end > start.Offset) {
			return false
		}
		slots[i].block, slots[i].start, slots[i].end = start.Block, start.Offset, end.Offset
	}
	s.slots, s.finish = slots, finish
	return true
}

func (a *App) selectWorkspaceTemplateField() {
	s := a.templateSession
	for i, slot := range s.slots {
		if slot.number != s.numbers[s.group] {
			continue
		}
		s.active = i
		if s.source != nil {
			a.selectWorkspaceSourceFields(slot.start, slot.end)
		} else {
			a.core.BreakCoalescing()
			a.focusAt(editor.Selection{
				Anchor: editor.Pos{Block: slot.block, Offset: slot.start}, Head: editor.Pos{Block: slot.block, Offset: slot.end},
			})
		}
		a.services.Invalidate()
		return
	}
}

func (a *App) endWorkspaceTemplateSession() {
	if s := a.templateSession; s != nil && s.source != nil && s.source == a.source {
		s.source.edit.SetSelections(s.source.edit.Selection())
	}
	a.templateSession = nil
}

func (a *App) workspaceTemplateKey(key ui.Key, mods ui.Modifiers) bool {
	s := a.templateSession
	if s == nil || s.outsideClick || s.source == nil && s.generation != a.core.Generation {
		return false
	}
	if key == ui.KeyEscape && mods == 0 {
		a.endWorkspaceTemplateSession()
		return true
	}
	if key != ui.KeyTab || mods&^ui.Shift != 0 {
		return false
	}
	if s.source == nil {
		sel := a.core.Sel
		if sel.Anchor.Block != sel.Head.Block || s.fieldAt(sel.Head.Block, min(sel.Anchor.Offset, sel.Head.Offset), max(sel.Anchor.Offset, sel.Head.Offset)) < 0 {
			a.endWorkspaceTemplateSession()
			return false
		}
	}
	if mods&ui.Shift != 0 {
		s.group = max(0, s.group-1)
	} else {
		s.group++
	}
	if s.group < len(s.numbers) {
		a.selectWorkspaceTemplateField()
		return true
	}
	a.endWorkspaceTemplateSession()
	switch {
	case s.source != nil:
		s.source.edit.Select(s.cursor, s.cursor)
	case s.finish.Block != 0:
		a.focusAt(editor.Caret(s.finish))
	default:
		a.focusWorkspaceTemplateCursor(len(a.markdownSource()) - s.tail)
	}
	a.services.Invalidate()
	return true
}

func (s *workspaceTemplateSession) fieldAt(block doc.BlockID, lo, hi int) int {
	if slot := s.slots[s.active]; slot.block == block && lo >= slot.start && hi <= slot.end {
		return s.active
	}
	for i, slot := range s.slots {
		if slot.number == s.numbers[s.group] && slot.block == block && lo >= slot.start && hi <= slot.end {
			return i
		}
	}
	return -1
}

func (a *App) observeWorkspaceTemplateField(b *doc.Block, p *blockBuffer, ev ui.InputEvent) {
	s := a.templateSession
	if s == nil || s.source != nil || ev.Kind != ui.InputPointerDown || ev.Button != 0 {
		return
	}
	at := a.modelOffset(b.ID, p.positionAt(ui.Point{X: ev.X, Y: ev.Y}))
	if i := s.fieldAt(b.ID, at, at); i >= 0 {
		s.active, s.outsideClick = i, false
	}
}

// A template edit must leave all text outside the active field intact.
func workspaceTemplateEditedValue(before, after string, lo, hi int) (string, bool) {
	if lo < 0 || hi < lo || hi > len(before) {
		return "", false
	}
	end := len(after) - (len(before) - hi)
	if end < lo || !strings.HasPrefix(after, before[:lo]) || !strings.HasSuffix(after, before[hi:]) {
		return "", false
	}
	return after[lo:end], true
}

func (a *App) syncWorkspaceTemplateField(b *doc.Block, p *blockBuffer, next doc.RichText) bool {
	s := a.templateSession
	if s == nil || s.source != nil || b.Text.Text == next.Text {
		return false
	}
	active := s.slots[s.active]
	value, ok := workspaceTemplateEditedValue(b.Text.Text, next.Text, active.start, active.end)
	if active.block != b.ID || s.generation != a.core.Generation || s.outsideClick || !ok {
		a.endWorkspaceTemplateSession()
		return false
	}
	changes := map[doc.BlockID]doc.RichText{b.ID: next}
	var beforeActive int
	for i := len(s.slots) - 1; i >= 0; i-- {
		slot := s.slots[i]
		if i == s.active || slot.number != active.number {
			continue
		}
		text, found := changes[slot.block]
		if !found {
			text = *a.core.TextOf(slot.block)
		}
		lo, hi := slot.start, slot.end
		if slot.block == b.ID {
			delta := len(value) - (active.end - active.start)
			if i > s.active {
				lo, hi = lo+delta, hi+delta
			} else {
				beforeActive += len(value) - (hi - lo)
			}
		}
		part := text.Slice(lo, hi)
		part.Resync(value)
		text.Replace(lo, hi, part)
		changes[slot.block] = text
	}
	anchor, head := p.native.Selection()
	anchor = doc.RuneToByte(next.Text, anchor) + beforeActive
	head = doc.RuneToByte(next.Text, head) + beforeActive
	// SyncRichText coalesces these writes into the same native typing step.
	a.core.SyncRichText(b.ID, changes[b.ID])
	delete(changes, b.ID)
	for _, slot := range s.slots {
		if text, ok := changes[slot.block]; ok {
			a.core.SyncRichText(slot.block, text)
			delete(changes, slot.block)
		}
	}
	s.resizeActiveGroup(len(value))
	s.generation = a.core.Generation
	a.ResetBuffers()
	p.native.SetSelection(doc.ByteToRune(b.Text.Text, anchor), doc.ByteToRune(b.Text.Text, head))
	a.readSelection(b, p)
	a.services.Invalidate()
	return true
}

func (s *workspaceTemplateSession) resizeActiveGroup(length int) {
	shift := make(map[doc.BlockID]int)
	finishBefore := s.finish.Offset
	if s.source != nil {
		finishBefore = s.cursor
	}
	for i, slot := range s.slots {
		field := &s.slots[i]
		field.start += shift[slot.block]
		field.end += shift[slot.block]
		if slot.number != s.numbers[s.group] {
			continue
		}
		delta := length - (slot.end - slot.start)
		field.end += delta
		shift[slot.block] += delta
		if finishBefore > slot.end || finishBefore == slot.end && i < s.cursorAfter {
			if s.source != nil {
				s.cursor += delta
			} else if slot.block == s.finish.Block {
				s.finish.Offset += delta
			}
		}
	}
}
