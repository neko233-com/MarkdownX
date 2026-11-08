// Package gitstore runs the handful of git commands used for document
// versioning. Every command receives arguments separately; no shell is used.
package gitstore

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"time"
)

var (
	ErrNoChanges = errors.New("nothing to commit")
	ErrUnsafeRev = errors.New("invalid revision")
	revPattern   = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._/~^-]{0,127}$`)
)

type Repo struct {
	root string
	name string
	mail string
	git  string
}

type Commit struct {
	Hash    string    `json:"hash"`
	Author  string    `json:"author"`
	Time    time.Time `json:"time"`
	Subject string    `json:"subject"`
}

func Open(root, name, mail string) (*Repo, error) {
	git, err := exec.LookPath("git")
	if err != nil {
		return nil, fmt.Errorf("git is required: %w", err)
	}
	if name == "" {
		name = "MarkdownX"
	}
	if mail == "" {
		mail = "server@markdownx.local"
	}
	r := &Repo{root: root, name: name, mail: mail, git: git}
	if err := r.ensure(context.Background()); err != nil {
		return nil, err
	}
	return r, nil
}

func (r *Repo) ensure(ctx context.Context) error {
	if err := os.MkdirAll(r.root, 0o755); err != nil {
		return err
	}
	// Check for a repository at root itself. `git rev-parse` would also succeed
	// when root is merely a subdirectory of another repository, and then every
	// commit would leak into the parent's history.
	if _, err := os.Stat(filepath.Join(r.root, ".git")); err != nil {
		if _, err := r.run(ctx, "init", "-b", "main"); err != nil {
			// Older git versions do not support -b; plain init is still fine.
			if _, err2 := r.run(ctx, "init"); err2 != nil {
				return fmt.Errorf("git init: %w", err)
			}
			_, _ = r.run(ctx, "symbolic-ref", "HEAD", "refs/heads/main")
		}
	}
	if _, err := r.run(ctx, "config", "user.name", r.name); err != nil {
		return err
	}
	if _, err := r.run(ctx, "config", "user.email", r.mail); err != nil {
		return err
	}
	ignore := filepath.Join(r.root, ".gitignore")
	if _, err := os.Stat(ignore); errors.Is(err, os.ErrNotExist) {
		_ = os.WriteFile(ignore, []byte("*.tmp\n.markdownx-*\n"), 0o644)
	}
	return nil
}

// Commit stages one document and commits only that path, so two rooms cannot
// accidentally commit each other's half-finished work.
func (r *Repo) Commit(ctx context.Context, rel, message, author string) (string, error) {
	if !safeRel(rel) {
		return "", ErrUnsafeRev
	}
	if strings.TrimSpace(message) == "" {
		message = "Update " + rel
	}
	if _, err := r.run(ctx, "add", "--", rel); err != nil {
		return "", err
	}
	// Exit 1 means there is a staged change; exit 0 means nothing to do.
	cmd := exec.CommandContext(ctx, r.git, "diff", "--cached", "--quiet", "--", rel)
	cmd.Dir = r.root
	err := cmd.Run()
	if err == nil {
		return "", ErrNoChanges
	}
	if exit, ok := err.(*exec.ExitError); !ok || exit.ExitCode() != 1 {
		return "", err
	}
	args := []string{"commit", "-m", message}
	if strings.TrimSpace(author) != "" {
		args = append(args, "--author", sanitizeAuthor(author)+" <"+r.mail+">")
	}
	args = append(args, "--", rel)
	if _, err := r.run(ctx, args...); err != nil {
		return "", err
	}
	out, err := r.run(ctx, "rev-parse", "HEAD")
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(out)), nil
}

func (r *Repo) Log(ctx context.Context, rel string, limit int) ([]Commit, error) {
	if !safeRel(rel) {
		return nil, ErrUnsafeRev
	}
	if limit < 1 {
		limit = 20
	}
	if limit > 200 {
		limit = 200
	}
	out, err := r.run(ctx, "log", "--follow", "--format=%H%x1f%an%x1f%aI%x1f%s", "--max-count", strconv.Itoa(limit), "--", rel)
	if err != nil {
		// An empty repository has no commits; that is not an API failure.
		if strings.Contains(err.Error(), "does not have any commits") || strings.Contains(err.Error(), "unknown revision") {
			return []Commit{}, nil
		}
		return nil, err
	}
	return parseLog(out)
}

func (r *Repo) Show(ctx context.Context, rev, rel string) ([]byte, error) {
	if !safeRel(rel) || !safeRev(rev) {
		return nil, ErrUnsafeRev
	}
	return r.run(ctx, "show", rev+":"+rel)
}

// Diff returns a readable unified diff. With no from revision it shows the
// selected commit (useful for the root commit); with neither it compares HEAD~1.
func (r *Repo) Diff(ctx context.Context, from, to, rel string) (string, error) {
	if !safeRel(rel) {
		return "", ErrUnsafeRev
	}
	if to == "" {
		to = "HEAD"
	}
	if !safeRev(to) {
		return "", ErrUnsafeRev
	}
	if from == "" {
		if _, err := r.run(ctx, "rev-parse", to+"^"); err == nil {
			from = to + "^"
		} else {
			out, err := r.run(ctx, "show", "--format=", "--no-color", to, "--", rel)
			return string(out), err
		}
	}
	if !safeRev(from) || !safeRev(to) {
		return "", ErrUnsafeRev
	}
	out, err := r.run(ctx, "diff", "--no-color", "--no-ext-diff", from, to, "--", rel)
	return string(out), err
}

// Merge performs a three-way text merge using git merge-file. It returns the
// merged bytes and whether the merge was clean; conflicted output still carries
// the familiar <<<<<<< markers so a human can resolve it.
func (r *Repo) Merge(ctx context.Context, base, current, incoming []byte) ([]byte, bool, error) {
	dir, err := os.MkdirTemp("", "markdownx-merge-")
	if err != nil {
		return nil, false, err
	}
	defer os.RemoveAll(dir)
	basePath := filepath.Join(dir, "base.md")
	currentPath := filepath.Join(dir, "current.md")
	incomingPath := filepath.Join(dir, "incoming.md")
	for path, body := range map[string][]byte{basePath: base, currentPath: current, incomingPath: incoming} {
		if err := os.WriteFile(path, body, 0o600); err != nil {
			return nil, false, err
		}
	}
	cmd := exec.CommandContext(ctx, r.git, "merge-file", "-p", currentPath, basePath, incomingPath)
	out, err := cmd.Output()
	if err == nil {
		return out, true, nil
	}
	if exit, ok := err.(*exec.ExitError); ok && exit.ExitCode() == 1 {
		return out, false, nil
	}
	return nil, false, fmt.Errorf("git merge-file: %w", err)
}

func (r *Repo) Head(ctx context.Context) (string, error) {
	out, err := r.run(ctx, "rev-parse", "HEAD")
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(out)), nil
}

func (r *Repo) run(ctx context.Context, args ...string) ([]byte, error) {
	cmd := exec.CommandContext(ctx, r.git, args...)
	cmd.Dir = r.root
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	out, err := cmd.Output()
	if err != nil {
		msg := strings.TrimSpace(stderr.String())
		if msg == "" {
			msg = err.Error()
		}
		return out, fmt.Errorf("git %s: %s", args[0], msg)
	}
	return out, nil
}

func parseLog(out []byte) ([]Commit, error) {
	var commits []Commit
	for _, line := range strings.Split(strings.TrimSpace(string(out)), "\n") {
		if line == "" {
			continue
		}
		parts := strings.SplitN(line, "\x1f", 4)
		if len(parts) != 4 {
			return nil, fmt.Errorf("unexpected git log line")
		}
		t, err := time.Parse(time.RFC3339, parts[2])
		if err != nil {
			return nil, err
		}
		commits = append(commits, Commit{Hash: parts[0], Author: parts[1], Time: t, Subject: parts[3]})
	}
	return commits, nil
}

func safeRel(rel string) bool {
	if rel == "" || strings.HasPrefix(rel, "/") || strings.Contains(rel, "\\") {
		return false
	}
	clean := filepath.ToSlash(filepath.Clean(rel))
	return clean == rel && clean != ".." && !strings.HasPrefix(clean, "../") && !strings.Contains(clean, "/../")
}

func safeRev(rev string) bool {
	return revPattern.MatchString(rev) && !strings.Contains(rev, "..") && !strings.Contains(rev, "@{")
}

func sanitizeAuthor(name string) string {
	name = strings.Map(func(r rune) rune {
		if r == '<' || r == '>' || r == '\n' || r == '\r' || r == 0 {
			return -1
		}
		return r
	}, name)
	name = strings.TrimSpace(name)
	if name == "" {
		return "MarkdownX"
	}
	if len([]rune(name)) > 64 {
		name = string([]rune(name)[:64])
	}
	return name
}
