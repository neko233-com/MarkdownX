// Package config holds the small set of server knobs that are safe to expose.
package config

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

const (
	DefaultAddr     = ":8787"
	DefaultMaxBytes = 2 << 20
)

type Config struct {
	Addr       string
	Root       string
	Token      string
	Origin     string
	MaxBytes   int64
	CommitName string
	CommitMail string
	WebDir     string
	Dev        bool
}

func FromFlags(args []string) (Config, error) {
	fs := flag.NewFlagSet("markdownx-server", flag.ContinueOnError)
	addr := fs.String("addr", env("MARKDOWNX_SERVER_ADDR", DefaultAddr), "listen address")
	root := fs.String("root", env("MARKDOWNX_SERVER_ROOT", "./data"), "workspace directory (created if missing)")
	token := fs.String("token", os.Getenv("MARKDOWNX_SERVER_TOKEN"), "bearer token; empty disables auth for local use")
	origin := fs.String("origin", os.Getenv("MARKDOWNX_SERVER_ORIGIN"), "allowed browser origin, e.g. https://books.example.com")
	maxBytes := fs.Int64("max-bytes", DefaultMaxBytes, "maximum document size in bytes")
	commitName := fs.String("git-name", env("MARKDOWNX_GIT_NAME", "MarkdownX"), "git author name")
	commitMail := fs.String("git-email", env("MARKDOWNX_GIT_EMAIL", "server@markdownx.local"), "git author email")
	webDir := fs.String("web", env("MARKDOWNX_SERVER_WEB", "web"), "browser editor directory")
	dev := fs.Bool("dev", false, "serve permissive CORS and verbose request logs")
	if err := fs.Parse(args); err != nil {
		return Config{}, err
	}
	abs, err := filepath.Abs(*root)
	if err != nil {
		return Config{}, fmt.Errorf("root: %w", err)
	}
	if *maxBytes < 1024 || *maxBytes > 64<<20 {
		return Config{}, fmt.Errorf("max-bytes must be between 1 KiB and 64 MiB")
	}
	return Config{
		Addr:       *addr,
		Root:       abs,
		Token:      *token,
		Origin:     *origin,
		MaxBytes:   *maxBytes,
		CommitName: *commitName,
		CommitMail: *commitMail,
		WebDir:     *webDir,
		Dev:        *dev,
	}, nil
}

func (c Config) ShutdownTimeout() time.Duration { return 8 * time.Second }

func env(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}
