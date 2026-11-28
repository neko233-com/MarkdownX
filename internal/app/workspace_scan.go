package app

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"unicode/utf8"

	"github.com/neko233/markdownx/internal/convert"
)

const workspaceSearchMaxHits = 500
const workspaceSearchMaxBytes = 16 << 20

type workspaceQuery struct {
	text       string
	names      bool
	matchCase  bool
	wholeWord  bool
	expression bool
}

type workspaceMatch struct {
	expression *regexp.Regexp
	wholeWord  bool
}

func (q workspaceQuery) compile() (workspaceMatch, error) {
	pattern := q.text
	if !q.expression {
		pattern = regexp.QuoteMeta(pattern)
	}
	if q.wholeWord {
		const boundary = `[^\p{L}\p{N}\p{M}_]`
		pattern = `(?:^|` + boundary + `)(` + pattern + `)(?:$|` + boundary + `)`
	}
	if !q.matchCase {
		pattern = "(?i)" + pattern
	}
	expression, err := regexp.Compile(pattern)
	return workspaceMatch{expression, q.wholeWord}, err
}

func (m workspaceMatch) find(text string) (start, end int, ok bool) {
	if m.wholeWord {
		at := m.expression.FindStringSubmatchIndex(text)
		if len(at) >= 4 && at[2] >= 0 {
			return at[2], at[3], true
		}
		return 0, 0, false
	}
	if at := m.expression.FindStringIndex(text); at != nil {
		return at[0], at[1], true
	}
	return 0, 0, false
}

type workspaceHit struct {
	relative string
	line     int
	excerpt  string
}

type workspaceSearchResult struct {
	hits    []workspaceHit
	capped  bool
	skipped int
	err     error
}

type workspaceLiveText struct {
	relative string
	content  string
}

func scanWorkspace(ctx context.Context, path string, files []string, query workspaceQuery, live workspaceLiveText) workspaceSearchResult {
	match, err := query.compile()
	if err != nil {
		return workspaceSearchResult{err: fmt.Errorf("搜索表达式无效：%w", err)}
	}
	result := workspaceSearchResult{}
	var root *os.Root
	if !query.names {
		root, err = os.OpenRoot(path)
		if err != nil {
			return workspaceSearchResult{err: err}
		}
		defer root.Close()
	}
	for _, relative := range files {
		if ctx.Err() != nil || result.capped {
			break
		}
		if query.names {
			name := filepath.Base(relative)
			if strings.Contains(query.text, "/") {
				name = filepath.ToSlash(relative)
			}
			if _, _, ok := match.find(name); ok {
				result.record(workspaceHit{relative: relative})
			}
			continue
		}
		if convert.Detect(relative) != convert.FormatMarkdown {
			continue
		}
		text := live.content
		if live.relative != relative {
			var ok bool
			text, ok = searchWorkspaceText(root, relative)
			if !ok {
				result.skipped++
				continue
			}
		} else if len(text) > workspaceSearchMaxBytes {
			result.skipped++
			continue
		}
		result.searchLines(ctx, relative, text, match)
	}
	return result
}

func openWorkspaceRegular(root *os.Root, relative string, expected os.FileInfo) (*os.File, error) {
	info, err := root.Lstat(relative)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || expected != nil && !os.SameFile(info, expected) {
		return nil, os.ErrInvalid
	}
	file, err := root.Open(relative)
	if err != nil {
		return nil, err
	}
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) {
		_ = file.Close()
		return nil, os.ErrInvalid
	}
	return file, nil
}

func searchWorkspaceText(root *os.Root, relative string) (string, bool) {
	file, err := openWorkspaceRegular(root, relative, nil)
	if err != nil {
		return "", false
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || info.Size() > workspaceSearchMaxBytes {
		return "", false
	}
	data, err := io.ReadAll(io.LimitReader(file, workspaceSearchMaxBytes+1))
	if err != nil || len(data) > workspaceSearchMaxBytes || !utf8.Valid(data) || bytes.ContainsRune(data, '\x00') {
		return "", false
	}
	return readMarkdownFile(relative, data).text, true
}

func (r *workspaceSearchResult) record(hit workspaceHit) {
	if len(r.hits) == workspaceSearchMaxHits {
		r.capped = true
		return
	}
	r.hits = append(r.hits, hit)
}

func (r *workspaceSearchResult) searchLines(ctx context.Context, relative, text string, match workspaceMatch) {
	number := 0
	for line := range strings.SplitSeq(text, "\n") {
		if ctx.Err() != nil || r.capped {
			return
		}
		number++
		if start, _, found := match.find(line); found {
			r.record(workspaceHit{
				relative: relative,
				line:     number,
				excerpt:  workspaceSearchExcerpt(line, start),
			})
		}
	}
}

func workspaceSearchExcerpt(line string, at int) string {
	left, right := max(0, at-56), min(len(line), at+104)
	for left > 0 && !utf8.RuneStart(line[left]) {
		left--
	}
	for right < len(line) && !utf8.RuneStart(line[right]) {
		right++
	}
	excerpt := strings.TrimSpace(line[left:right])
	if left != 0 {
		excerpt = "…" + excerpt
	}
	if right != len(line) {
		excerpt += "…"
	}
	return excerpt
}
