package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/neko233-com/MarkdownX/server/internal/gitstore"
	"github.com/neko233-com/MarkdownX/server/internal/store"
)

const (
	historyLimit = 256
	TypeSnapshot = "snapshot"
	TypeUpdate   = "update"
	TypeConflict = "conflict"
	TypePresence = "presence"
	TypeCursor   = "cursor"
	TypeError    = "error"
	TypeSaved    = "saved"
)

var (
	ErrStaleBase = errors.New("revision is no longer available")
	ErrTooBig    = errors.New("document is too large")
)

type Revision struct {
	Rev     int64
	Content string
	By      string
	At      time.Time
}

type Peer struct {
	ID     string `json:"id"`
	Name   string `json:"name"`
	Color  string `json:"color"`
	Cursor int    `json:"cursor"`
}

type Snapshot struct {
	Type     string `json:"type"`
	ClientID string `json:"client_id"`
	ID       string `json:"id"`
	Title    string `json:"title"`
	Rev      int64  `json:"rev"`
	Content  string `json:"content"`
	Peers    []Peer `json:"peers"`
}

type Update struct {
	Type    string `json:"type"`
	ID      string `json:"id"`
	Rev     int64  `json:"rev"`
	Title   string `json:"title"`
	Content string `json:"content"`
	By      string `json:"by"`
	Origin  string `json:"origin,omitempty"`
	At      string `json:"at"`
}

type Conflict struct {
	Type    string `json:"type"`
	ID      string `json:"id"`
	Rev     int64  `json:"rev"`
	Title   string `json:"title"`
	Content string `json:"content"`
	Merged  string `json:"merged,omitempty"`
	By      string `json:"by"`
}

type SaveOutcome struct {
	Rev      int64
	Content  string
	Title    string
	By       string
	At       time.Time
	Changed  bool
	Commit   string
	GitError string
	Conflict *Conflict
}

type Room struct {
	id       string
	store    *store.Store
	repo     *gitstore.Repo
	maxBytes int64

	mu      sync.Mutex
	content string
	rev     int64
	by      string
	at      time.Time
	history []Revision
	clients map[*Client]struct{}
}

func NewRoom(id string, st *store.Store, repo *gitstore.Repo, maxBytes int64) (*Room, error) {
	b, err := st.Read(id)
	if errors.Is(err, store.ErrNoDoc) {
		b = []byte{}
	} else if err != nil {
		return nil, err
	}
	now := time.Now().UTC()
	return &Room{
		id:       id,
		store:    st,
		repo:     repo,
		maxBytes: maxBytes,
		content:  string(b),
		rev:      1,
		at:       now,
		history:  []Revision{{Rev: 1, Content: string(b), At: now}},
		clients:  make(map[*Client]struct{}),
	}, nil
}

func (r *Room) ID() string { return r.id }

func (r *Room) Snapshot(clientID string) Snapshot {
	r.mu.Lock()
	defer r.mu.Unlock()
	return Snapshot{
		Type:     TypeSnapshot,
		ClientID: clientID,
		ID:       r.id,
		Title:    store.Title(r.content, r.id),
		Rev:      r.rev,
		Content:  r.content,
		Peers:    r.peersLocked(),
	}
}

func (r *Room) Current() (rev int64, content, title, by string, at time.Time) {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.rev, r.content, store.Title(r.content, r.id), r.by, r.at
}

// Save accepts a complete document revision. If the supplied base is stale the
// room tries a three-way merge against that snapshot; if the merge conflicts,
// the caller receives the current document and the conflicted merge text and
// nothing is written.
func (r *Room) Save(ctx context.Context, base int64, content, author, origin string) (SaveOutcome, error) {
	if int64(len(content)) > r.maxBytes {
		return SaveOutcome{}, ErrTooBig
	}
	if author == "" {
		author = "anonymous"
	}
	now := time.Now().UTC()

	r.mu.Lock()
	if base == r.rev {
		if content == r.content {
			out := SaveOutcome{Rev: r.rev, Content: r.content, Title: store.Title(r.content, r.id), By: r.by, At: r.at}
			r.mu.Unlock()
			return out, nil
		}
		return r.commitLocked(ctx, base, content, author, origin, now)
	}
	if base > r.rev {
		r.mu.Unlock()
		return SaveOutcome{}, ErrStaleBase
	}
	baseContent, ok := r.revisionLocked(base)
	if !ok {
		r.mu.Unlock()
		return SaveOutcome{}, ErrStaleBase
	}
	current := r.content
	expected := r.rev
	r.mu.Unlock()

	merged, clean, err := r.repo.Merge(ctx, []byte(baseContent), []byte(current), []byte(content))
	if err != nil {
		return SaveOutcome{}, fmt.Errorf("merge: %w", err)
	}
	if !clean {
		r.mu.Lock()
		conflictRev := r.rev
		current = r.content
		r.mu.Unlock()
		conflict := &Conflict{
			Type:    TypeConflict,
			ID:      r.id,
			Rev:     conflictRev,
			Title:   store.Title(current, r.id),
			Content: current,
			Merged:  string(merged),
			By:      author,
		}
		return SaveOutcome{Rev: conflictRev, Content: current, Title: conflict.Title, By: author, At: now, Conflict: conflict}, nil
	}
	if string(merged) == current {
		return SaveOutcome{Rev: expected, Content: current, Title: store.Title(current, r.id), By: author, At: now}, nil
	}
	r.mu.Lock()
	if r.rev != expected {
		current = r.content
		r.mu.Unlock()
		return SaveOutcome{Rev: expected, Content: current, Title: store.Title(current, r.id), By: author, At: now, Conflict: &Conflict{
			Type: TypeConflict, ID: r.id, Rev: expected, Title: store.Title(current, r.id), Content: current, By: author,
		}}, nil
	}
	return r.commitLocked(ctx, expected, string(merged), author, origin, now)
}

// commitLocked must be called with r.mu held. Writing the small Markdown file
// while holding the room lock makes the revision check and the write atomic
// with respect to other editors.
func (r *Room) commitLocked(ctx context.Context, expected int64, content, author, origin string, now time.Time) (SaveOutcome, error) {
	if r.rev != expected {
		r.mu.Unlock()
		return SaveOutcome{}, ErrStaleBase
	}
	if int64(len(content)) > r.maxBytes {
		r.mu.Unlock()
		return SaveOutcome{}, ErrTooBig
	}
	if err := r.store.Write(r.id, []byte(content), r.maxBytes); err != nil {
		r.mu.Unlock()
		return SaveOutcome{}, err
	}
	r.content = content
	r.rev++
	r.by = author
	r.at = now
	r.history = append(r.history, Revision{Rev: r.rev, Content: content, By: author, At: now})
	if len(r.history) > historyLimit {
		r.history = append([]Revision(nil), r.history[len(r.history)-historyLimit:]...)
	}
	update := Update{
		Type:    TypeUpdate,
		ID:      r.id,
		Rev:     r.rev,
		Title:   store.Title(content, r.id),
		Content: content,
		By:      author,
		Origin:  origin,
		At:      now.Format(time.RFC3339),
	}
	wire, _ := json.Marshal(update)
	clients := r.clientsLocked()
	hash, gitErr := r.commitGit(ctx, author)
	r.mu.Unlock()

	for _, c := range clients {
		c.Push(wire)
	}
	out := SaveOutcome{Rev: update.Rev, Content: content, Title: update.Title, By: author, At: now, Changed: true, Commit: hash}
	if gitErr != nil {
		out.GitError = gitErr.Error()
	}
	return out, nil
}

func (r *Room) commitGit(ctx context.Context, author string) (string, error) {
	rel, err := r.store.Rel(r.id)
	if err != nil {
		return "", err
	}
	msg := "edit: " + author
	return r.repo.Commit(ctx, rel, msg, author)
}

// Restore replaces the working document with a known revision and records it
// as a new revision so clients converge on the restored text.
func (r *Room) Restore(content, author, origin string) (SaveOutcome, error) {
	if int64(len(content)) > r.maxBytes {
		return SaveOutcome{}, ErrTooBig
	}
	if author == "" {
		author = "anonymous"
	}
	r.mu.Lock()
	return r.commitLocked(context.Background(), r.rev, content, author, origin, time.Now().UTC())
}

func (r *Room) revisionLocked(rev int64) (string, bool) {
	for i := len(r.history) - 1; i >= 0; i-- {
		if r.history[i].Rev == rev {
			return r.history[i].Content, true
		}
	}
	return "", false
}

func (r *Room) join(c *Client) Snapshot {
	r.mu.Lock()
	r.clients[c] = struct{}{}
	snap := Snapshot{
		Type:     TypeSnapshot,
		ClientID: c.ID,
		ID:       r.id,
		Title:    store.Title(r.content, r.id),
		Rev:      r.rev,
		Content:  r.content,
		Peers:    r.peersLocked(),
	}
	r.mu.Unlock()
	r.broadcastPresence()
	return snap
}

func (r *Room) leave(c *Client) {
	r.mu.Lock()
	delete(r.clients, c)
	r.mu.Unlock()
	r.broadcastPresence()
}

func (r *Room) setCursor(c *Client, pos int) {
	if pos < 0 {
		pos = 0
	}
	r.mu.Lock()
	c.Cursor = pos
	peers := r.peersLocked()
	r.mu.Unlock()
	msg, _ := json.Marshal(map[string]any{"type": TypeCursor, "id": c.ID, "name": c.Name, "cursor": pos, "peers": peers})
	r.broadcast(msg)
}

func (r *Room) broadcastPresence() {
	r.mu.Lock()
	msg, _ := json.Marshal(map[string]any{"type": TypePresence, "peers": r.peersLocked()})
	clients := r.clientsLocked()
	r.mu.Unlock()
	for _, c := range clients {
		c.Push(msg)
	}
}

func (r *Room) broadcast(msg []byte) {
	r.mu.Lock()
	clients := r.clientsLocked()
	r.mu.Unlock()
	for _, c := range clients {
		c.Push(msg)
	}
}

func (r *Room) clientsLocked() []*Client {
	out := make([]*Client, 0, len(r.clients))
	for c := range r.clients {
		out = append(out, c)
	}
	return out
}

func (r *Room) peersLocked() []Peer {
	out := make([]Peer, 0, len(r.clients))
	for c := range r.clients {
		out = append(out, Peer{ID: c.ID, Name: c.Name, Color: c.Color, Cursor: c.Cursor})
	}
	return out
}

func cleanName(name string) string {
	name = strings.TrimSpace(name)
	name = strings.Map(func(r rune) rune {
		if r == '\n' || r == '\r' || r == '\t' || r == 0 {
			return -1
		}
		return r
	}, name)
	if name == "" {
		return "Guest"
	}
	runes := []rune(name)
	if len(runes) > 32 {
		name = string(runes[:32])
	}
	return name
}
