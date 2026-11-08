// Command markdownx-server serves a small, self-hosted collaborative Markdown
// workspace. Documents live as plain .md files under <root>/docs and every
// accepted edit is committed to the Git repository in <root>.
package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/neko233-com/MarkdownX/server/internal/api"
	"github.com/neko233-com/MarkdownX/server/internal/config"
	"github.com/neko233-com/MarkdownX/server/internal/gitstore"
	"github.com/neko233-com/MarkdownX/server/internal/live"
	"github.com/neko233-com/MarkdownX/server/internal/store"
)

func main() {
	if err := run(); err != nil {
		log.Fatal(err)
	}
}

func run() error {
	cfg, err := config.FromFlags(os.Args[1:])
	if err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return err
	}
	if cfg.Token == "" && !cfg.Dev {
		cfg.Token = randomToken()
		log.Printf("MARKDOWNX_SERVER_TOKEN not set; generated token: %s", cfg.Token)
	}
	if cfg.Root == "" {
		return errors.New("root is required")
	}
	web, err := filepath.Abs(cfg.WebDir)
	if err != nil {
		return fmt.Errorf("web dir: %w", err)
	}
	if _, err := os.Stat(filepath.Join(web, "index.html")); err != nil {
		return fmt.Errorf("web editor not found at %s: %w", web, err)
	}

	st, err := store.Open(cfg.Root)
	if err != nil {
		return err
	}
	repo, err := gitstore.Open(cfg.Root, cfg.CommitName, cfg.CommitMail)
	if err != nil {
		return err
	}
	hub := live.NewHub(st, repo, cfg.MaxBytes, cfg.Origin, cfg.Dev)
	apiHandler := api.New(cfg, st, repo, hub).Handler()

	mux := http.NewServeMux()
	mux.Handle("/api/", apiHandler)
	mux.Handle("/ws/", apiHandler)
	mux.Handle("/", noCache(http.FileServer(http.Dir(web))))

	srv := &http.Server{
		Addr:              cfg.Addr,
		Handler:           logRequests(cfg.Dev, mux),
		ReadHeaderTimeout: 5 * time.Second,
		IdleTimeout:       90 * time.Second,
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go func() {
		<-ctx.Done()
		shutdown, cancel := context.WithTimeout(context.Background(), cfg.ShutdownTimeout())
		defer cancel()
		_ = srv.Shutdown(shutdown)
	}()

	log.Printf("MarkdownX server listening on %s (root %s, git %s)", cfg.Addr, cfg.Root, cfg.CommitName)
	if err := srv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		return err
	}
	return nil
}

func randomToken() string {
	b := make([]byte, 24)
	if _, err := rand.Read(b); err != nil {
		return fmt.Sprintf("%d", time.Now().UnixNano())
	}
	return hex.EncodeToString(b)
}

func noCache(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/" || r.URL.Path == "/index.html" {
			w.Header().Set("Cache-Control", "no-store")
		}
		next.ServeHTTP(w, r)
	})
}

func logRequests(dev bool, next http.Handler) http.Handler {
	if !dev {
		return next
	}
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		next.ServeHTTP(w, r)
		log.Printf("%s %s %s", r.Method, r.URL.Path, time.Since(start).Round(time.Millisecond))
	})
}
