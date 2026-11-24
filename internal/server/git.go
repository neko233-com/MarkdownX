package server

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// git runs a git command inside the workspace.
func git(root string, args ...string) (string, error) {
	cmd := exec.Command("git", append([]string{"-C", root}, args...)...)
	var out, errb strings.Builder
	cmd.Stdout = &out
	cmd.Stderr = &errb
	if err := cmd.Run(); err != nil {
		msg := strings.TrimSpace(errb.String())
		if msg == "" {
			msg = err.Error()
		}
		return out.String(), fmt.Errorf("git %s: %s", strings.Join(args, " "), msg)
	}
	return out.String(), nil
}

// IsRepo reports whether root is inside a git work tree.
func IsRepo(root string) bool {
	_, err := git(root, "rev-parse", "--is-inside-work-tree")
	return err == nil
}

// InitRepo creates a repository with one empty commit, so history works
// before the first save.
func InitRepo(root string) error {
	if IsRepo(root) {
		return nil
	}
	if _, err := git(root, "init"); err != nil {
		return err
	}
	if _, err := git(root, "config", "user.name", "MarkdownX"); err != nil {
		return err
	}
	if _, err := git(root, "config", "user.email", "markdownx@localhost"); err != nil {
		return err
	}
	readme := "MarkdownX workspace\n"
	marker := filepath.Join(root, ".markdownx-workspace")
	if err := os.WriteFile(marker, []byte(readme), 0o644); err != nil {
		return err
	}
	if _, err := git(root, "add", ".markdownx-workspace"); err != nil {
		return err
	}
	_, err := git(root, "commit", "-m", "MarkdownX workspace")
	return err
}

// Commit records one file's change.
func Commit(root, rel, message string) (string, error) {
	if _, err := git(root, "add", "--", rel); err != nil {
		return "", err
	}
	// Nothing staged is not an error: the file may already match HEAD.
	if _, err := git(root, "commit", "-m", message, "--", rel); err != nil {
		if strings.Contains(err.Error(), "nothing to commit") ||
			strings.Contains(err.Error(), "no changes added to commit") {
			return Head(root, rel), nil
		}
		return "", err
	}
	return Head(root, rel), nil
}

// Head is the newest commit that touched a file, or "" when it is untracked.
func Head(root, rel string) string {
	out, err := git(root, "log", "-1", "--format=%H", "--", rel)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(out)
}

// Revision is one history entry.
type Revision struct {
	Hash    string `json:"hash"`
	Time    string `json:"time"`
	Author  string `json:"author"`
	Subject string `json:"subject"`
}

// History lists a file's commits, newest first.
func History(root, rel string, limit int) ([]Revision, error) {
	if limit <= 0 {
		limit = 50
	}
	out, err := git(root, "log", fmt.Sprintf("-n%d", limit),
		"--format=%H%x1f%aI%x1f%an%x1f%s", "--", rel)
	if err != nil {
		return nil, err
	}
	list := []Revision{}
	for _, line := range strings.Split(strings.TrimSpace(out), "\n") {
		if line == "" {
			continue
		}
		parts := strings.SplitN(line, "\x1f", 4)
		if len(parts) != 4 {
			continue
		}
		list = append(list, Revision{Hash: parts[0], Time: parts[1], Author: parts[2], Subject: parts[3]})
	}
	return list, nil
}

// ShowFile returns a file's content at one commit.
func ShowFile(root, rel, commit string) (string, error) {
	if commit == "" {
		commit = "HEAD"
	}
	out, err := git(root, "show", commit+":"+rel)
	if err != nil {
		return "", err
	}
	return out, nil
}

// Diff is a commit's change to one file.
func Diff(root, rel, commit string) (string, error) {
	out, err := git(root, "show", "--format=", "--no-color", commit, "--", rel)
	if err != nil {
		return "", err
	}
	return out, nil
}

// Restore writes a file back as it was at one commit, and commits that.
func Restore(root, rel, commit string) error {
	content, err := ShowFile(root, rel, commit)
	if err != nil {
		return err
	}
	if err := WriteFile(root, rel, content); err != nil {
		return err
	}
	_, err = Commit(root, rel, "restore "+rel+" to "+short(commit))
	return err
}

func short(hash string) string {
	if len(hash) > 8 {
		return hash[:8]
	}
	return hash
}
