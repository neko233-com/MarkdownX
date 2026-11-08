// Package live owns document rooms and the WebSocket protocol. The protocol is
// intentionally small: a client joins a document, receives a snapshot, then
// sends complete revisions with the base revision it started from.
package live

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/gorilla/websocket"
	"github.com/neko233-com/MarkdownX/server/internal/gitstore"
	"github.com/neko233-com/MarkdownX/server/internal/store"
)

var palette = []string{"#b05226", "#2f6f5e", "#5b5f97", "#a23e48", "#5c7a29", "#8a5a2b", "#3d6b8e", "#7a4f8a"}

type Hub struct {
	store    *store.Store
	repo     *gitstore.Repo
	maxBytes int64
	origin   string
	dev      bool

	mu    sync.Mutex
	rooms map[string]*Room
}

func NewHub(st *store.Store, repo *gitstore.Repo, maxBytes int64, origin string, dev bool) *Hub {
	return &Hub{store: st, repo: repo, maxBytes: maxBytes, origin: origin, dev: dev, rooms: make(map[string]*Room)}
}

func (h *Hub) Room(id string) (*Room, error) {
	if !store.ValidID(id) {
		return nil, store.ErrBadID
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	if room, ok := h.rooms[id]; ok {
		return room, nil
	}
	room, err := NewRoom(id, h.store, h.repo, h.maxBytes)
	if err != nil {
		return nil, err
	}
	h.rooms[id] = room
	return room, nil
}

func (h *Hub) ServeWS(w http.ResponseWriter, r *http.Request, docID string) {
	room, err := h.Room(docID)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	up := websocket.Upgrader{
		ReadBufferSize:  4096,
		WriteBufferSize: 8192,
		CheckOrigin:     h.checkOrigin,
	}
	conn, err := up.Upgrade(w, r, nil)
	if err != nil {
		return
	}
	conn.SetReadLimit(h.maxBytes + 64*1024)
	client := &Client{
		ID:     randomID(8),
		Name:   cleanName(r.URL.Query().Get("name")),
		Color:  palette[bucket(r.URL.Query().Get("name"), len(palette))],
		conn:   conn,
		room:   room,
		send:   make(chan []byte, 64),
		done:   make(chan struct{}),
	}
	if client.Name == "Guest" {
		client.Name = "Guest-" + client.ID[:4]
	}
	snap := room.join(client)
	_ = conn.SetWriteDeadline(time.Now().Add(10 * time.Second))
	if err := conn.WriteJSON(snap); err != nil {
		room.leave(client)
		conn.Close()
		return
	}
	go client.writePump()
	client.readPump(h)
}

func (h *Hub) checkOrigin(r *http.Request) bool {
	if h.dev || h.origin == "*" {
		return true
	}
	origin := r.Header.Get("Origin")
	if origin == "" {
		return true // non-browser client
	}
	if h.origin != "" {
		return strings.TrimRight(origin, "/") == strings.TrimRight(h.origin, "/")
	}
	u, err := url.Parse(origin)
	if err != nil {
		return false
	}
	return strings.EqualFold(u.Host, r.Host)
}

type Client struct {
	ID     string
	Name   string
	Color  string
	Cursor int

	conn *websocket.Conn
	room *Room
	send chan []byte
	done chan struct{}

	closeOnce sync.Once
	writeMu   sync.Mutex
}

func (c *Client) Push(msg []byte) {
	select {
	case <-c.done:
	case c.send <- msg:
	default:
		// A slow reader must not block the room. Closing makes the browser
		// reconnect and fetch a fresh snapshot.
		c.close()
	}
}

func (c *Client) close() {
	c.closeOnce.Do(func() {
		close(c.done)
		_ = c.conn.Close()
	})
}

func (c *Client) writePump() {
	ticker := time.NewTicker(30 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-c.done:
			return
		case msg := <-c.send:
			if err := c.writeRaw(websocket.TextMessage, msg); err != nil {
				c.close()
				return
			}
		case <-ticker.C:
			if err := c.writeRaw(websocket.PingMessage, nil); err != nil {
				c.close()
				return
			}
		}
	}
}

func (c *Client) writeRaw(kind int, data []byte) error {
	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	_ = c.conn.SetWriteDeadline(time.Now().Add(10 * time.Second))
	return c.conn.WriteMessage(kind, data)
}

type incoming struct {
	Type    string `json:"type"`
	Base    int64  `json:"base"`
	Content string `json:"content"`
	Name    string `json:"name"`
	Cursor  int    `json:"cursor"`
}

func (c *Client) readPump(h *Hub) {
	defer func() {
		c.room.leave(c)
		c.close()
	}()
	c.conn.SetReadLimit(h.maxBytes + 64*1024)
	_ = c.conn.SetReadDeadline(time.Now().Add(60 * time.Second))
	c.conn.SetPongHandler(func(string) error {
		return c.conn.SetReadDeadline(time.Now().Add(60 * time.Second))
	})
	for {
		var msg incoming
		if err := c.conn.ReadJSON(&msg); err != nil {
			return
		}
		_ = c.conn.SetReadDeadline(time.Now().Add(60 * time.Second))
		switch msg.Type {
		case "save":
			if msg.Base < 0 {
				c.sendError("base must be >= 0")
				continue
			}
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			out, err := c.room.Save(ctx, msg.Base, msg.Content, c.Name, c.ID)
			cancel()
			if err != nil {
				c.sendError(err.Error())
				continue
			}
			if out.Conflict != nil {
				c.sendJSON(out.Conflict)
				continue
			}
			// The room broadcast already carries the update. This ack makes
			// the sender's revision advance even when the save was a no-op.
			c.sendJSON(map[string]any{"type": TypeSaved, "rev": out.Rev, "changed": out.Changed, "title": out.Title})
		case "cursor":
			c.room.setCursor(c, msg.Cursor)
		case "ping":
			c.sendJSON(map[string]any{"type": "pong", "at": time.Now().UTC().Format(time.RFC3339)})
		default:
			c.sendError("unknown message type")
		}
	}
}

func (c *Client) sendError(text string) {
	c.sendJSON(map[string]any{"type": TypeError, "message": text})
}

func (c *Client) sendJSON(v any) {
	b, err := json.Marshal(v)
	if err != nil {
		return
	}
	c.Push(b)
}

func randomID(n int) string {
	b := make([]byte, n/2+1)
	if _, err := rand.Read(b); err != nil {
		return fmt.Sprintf("%d", time.Now().UnixNano())
	}
	return hex.EncodeToString(b)[:n]
}

func bucket(s string, n int) int {
	if n <= 1 {
		return 0
	}
	var h uint32
	for _, r := range s {
		h = h*31 + uint32(r)
	}
	return int(h % uint32(n))
}
