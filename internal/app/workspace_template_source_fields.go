package app

import (
	"github.com/egoist/mygo/ui"
	sourceeditor "github.com/egoist/mygo/ui/editor"
)

func (a *App) selectWorkspaceSourceFields(start, end int) {
	s := a.templateSession
	active := s.slots[s.active]
	var selections []sourceeditor.Selection
	for i, slot := range s.slots {
		if slot.number == active.number && i != s.active {
			shift := slot.start - active.start
			selections = append(selections, sourceeditor.Selection{Anchor: start + shift, Head: end + shift})
		}
	}
	// The editor follows the newest selection for scrolling and IME.
	selections = append(selections, sourceeditor.Selection{Anchor: start, Head: end})
	s.source.edit.SetSelections(selections...)
	s.mergedSource = len(s.source.edit.Selections()) < len(selections)
	if s.mergedSource {
		// Edit just the primary field so replay can fill every mirror once.
		s.source.edit.Select(start, end)
	}
}

func (a *App) workspaceTemplateSourceInput(source *sourceView, ev ui.InputEvent) bool {
	s := a.templateSession
	if s == nil || s.source != source {
		return false
	}
	switch ev.Kind {
	case ui.InputText:
		if s.mergedSource {
			source.edit.Buffer.Transact(func() {
				source.edit.Insert(ev.Text)
				a.syncWorkspaceTemplateSource(source)
			})
			return true
		}
	case ui.InputPointerDown:
		s.sourceClick = ev.Button == 0
	case ui.InputCommand:
		if ev.Text == "undo" || ev.Text == "redo" {
			a.endWorkspaceTemplateSession()
		}
		if s.mergedSource && (ev.Text == "paste" || ev.Text == "pasteAndMatchStyle") {
			return a.pasteWorkspaceSourceField(source)
		}
	case ui.InputKeyDown:
		a.syncWorkspaceTemplateSource(source)
		if a.templateSession == s && s.mergedSource && ev.Key == ui.KeyV && ev.Mods == ui.Cmd {
			return a.pasteWorkspaceSourceField(source)
		}
		return a.workspaceTemplateKey(ev.Key, ev.Mods)
	}
	return false
}

func (a *App) pasteWorkspaceSourceField(source *sourceView) bool {
	source.edit.Buffer.Transact(func() {
		source.edit.Run(sourceeditor.Paste)
		a.syncWorkspaceTemplateSource(source)
	})
	return true
}

func (a *App) syncWorkspaceTemplateSource(source *sourceView) {
	s := a.templateSession
	if s == nil || s.source != source {
		return
	}
	if next := source.text(); next != s.text {
		if (s.mergedSource || !s.acceptSourceEdit(next)) && !a.mirrorWorkspaceSourceField(s, next) {
			a.endWorkspaceTemplateSession()
			return
		}
	}
	if !s.sourceClick {
		return
	}
	s.sourceClick = false
	sel := source.edit.Selection()
	if i := s.fieldAt(0, sel.Start(), sel.End()); i >= 0 {
		s.active, s.outsideClick = i, false
		a.selectWorkspaceSourceFields(sel.Anchor, sel.Head)
	}
}

// Coincident empty fields share a single caret in MyGo. A native edit at
// that caret is replayed into the other members of the template group.
func (a *App) mirrorWorkspaceSourceField(s *workspaceTemplateSession, next string) bool {
	active := s.slots[s.active]
	value, ok := workspaceTemplateEditedValue(s.text, next, active.start, active.end)
	sel := s.source.edit.Selection()
	if !ok || sel.Start() < active.start || sel.End() > active.start+len(value) {
		return false
	}
	var edits []sourceeditor.Edit
	before := 0
	for i, slot := range s.slots {
		if i == s.active || slot.number != active.number {
			continue
		}
		start, end := slot.start, slot.end
		if i > s.active {
			delta := len(value) - (active.end - active.start)
			start, end = start+delta, end+delta
		} else {
			before += len(value) - (end - start)
		}
		edits = append(edits, sourceeditor.Edit{Start: start, End: end, Text: value})
	}
	s.source.edit.Buffer.Edit(edits...)
	s.resizeActiveGroup(len(value))
	s.text = s.source.text()
	a.selectWorkspaceSourceFields(sel.Anchor+before, sel.Head+before)
	return true
}

// The code editor makes one edit at every selection of a group. Keep the
// ranges only when all replicas still agree and their surroundings are intact.
func (s *workspaceTemplateSession) acceptSourceEdit(next string) bool {
	active := s.slots[s.active]
	count := 0
	for _, slot := range s.slots {
		if slot.number == active.number {
			count++
		}
	}
	delta := len(next) - len(s.text)
	length := active.end - active.start + delta/count
	if delta%count != 0 || length < 0 {
		return false
	}
	oldAt, newAt := 0, 0
	value, first := "", true
	for _, slot := range s.slots {
		if slot.number != active.number {
			continue
		}
		start := newAt + slot.start - oldAt
		end := start + length
		if end > len(next) || next[newAt:start] != s.text[oldAt:slot.start] || (!first && value != next[start:end]) {
			return false
		}
		value, first = next[start:end], false
		oldAt, newAt = slot.end, end
	}
	if s.text[oldAt:] != next[newAt:] {
		return false
	}
	s.resizeActiveGroup(length)
	s.text = next
	if len(s.source.edit.Selections()) < count {
		// Clearing adjacent fields can leave coincident carets after a native edit.
		s.mergedSource = true
		s.source.edit.SetSelections(s.source.edit.Selection())
	}
	return true
}
