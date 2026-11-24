package doc

import "testing"

func TestResyncKeepsMarksAroundAnInsertion(t *testing.T) {
	text := PlainRich("hello ")
	text.Push("bold", Marks{Bold: true})
	text.Push(" world", Marks{})

	// Type inside the plain prefix.
	text.Resync("hey hello bold world")
	if text.Text != "hey hello bold world" {
		t.Fatalf("text = %q", text.Text)
	}
	var bold string
	for _, sp := range text.Spans() {
		if sp.Marks.Bold {
			bold += text.Text[sp.Start:sp.End]
		}
	}
	if bold != "bold" {
		t.Fatalf("bold span = %q in %#v", bold, text.Runs)
	}
}

func TestResyncInsertionTakesMarksAtTheCaret(t *testing.T) {
	text := PlainRich("ab")
	text.Push("cd", Marks{Bold: true})
	text.Push("ef", Marks{})

	// Insert in the middle of the bold run: the new text inherits bold.
	text.Resync("ab cdef")
	if text.Text != "ab cdef" {
		t.Fatalf("text = %q", text.Text)
	}
	at := len("ab ")
	var got Marks
	for _, sp := range text.Spans() {
		if sp.Start <= at && at < sp.End {
			got = sp.Marks
		}
	}
	if !got.Bold {
		t.Fatalf("inserted text is not bold: %#v", text.Runs)
	}
}

func TestResyncDeletionKeepsTheRest(t *testing.T) {
	text := PlainRich("keep ")
	text.Push("gone", Marks{Italic: true})
	text.Push(" tail", Marks{Bold: true})

	text.Resync("keep  tail")
	if text.Text != "keep  tail" {
		t.Fatalf("text = %q", text.Text)
	}
	var boldTail bool
	for _, sp := range text.Spans() {
		if sp.Marks.Bold && text.Text[sp.Start:sp.End] == " tail" {
			boldTail = true
		}
	}
	if !boldTail {
		t.Fatalf("bold tail lost: %#v", text.Runs)
	}
	for _, sp := range text.Spans() {
		if sp.Marks.Italic {
			t.Fatalf("deleted italic run survived: %#v", text.Runs)
		}
	}
}

func TestResyncToEmpty(t *testing.T) {
	text := PlainRich("abc")
	text.Push("d", Marks{Bold: true})
	text.Resync("")
	if text.Text != "" || len(text.Runs) != 0 {
		t.Fatalf("text = %q runs = %#v", text.Text, text.Runs)
	}
}

func TestResyncNoOpWhenUnchanged(t *testing.T) {
	text := PlainRich("same")
	before := len(text.Runs)
	text.Resync("same")
	if text.Text != "same" || len(text.Runs) != before {
		t.Fatalf("changed: %#v", text)
	}
}

func TestRuneByteOffsets(t *testing.T) {
	s := "中a文b"
	cases := []struct {
		rune, byte int
	}{
		{rune: 0, byte: 0},
		{rune: 1, byte: 3},
		{rune: 2, byte: 4},
		{rune: 3, byte: 7},
		{rune: 4, byte: 8},
		{rune: 99, byte: 8},
	}
	for _, c := range cases {
		if got := RuneToByte(s, c.rune); got != c.byte {
			t.Errorf("RuneToByte(%d) = %d, want %d", c.rune, got, c.byte)
		}
		if got := ByteToRune(s, c.byte); got != c.rune && c.rune < 5 {
			t.Errorf("ByteToRune(%d) = %d, want %d", c.byte, got, c.rune)
		}
	}
	// A byte in the middle of a rune falls back to its start.
	if got := ByteToRune(s, 1); got != 0 {
		t.Errorf("ByteToRune(1) = %d, want 0", got)
	}
}
