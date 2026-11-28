package doc

import (
	"net/url"
	"strings"
	"unicode"
)

// LocalFileURLPath returns a decoded, slash-rooted path from a local file URL.
// The caller interprets drive letters using the host filesystem.
func LocalFileURLPath(link string) (string, bool) {
	if len(link) < len("file:") || !strings.EqualFold(link[:len("file:")], "file:") {
		return "", false
	}
	u, err := url.Parse(link)
	if err != nil || u.Scheme != "file" || u.User != nil || u.Opaque != "" || u.ForceQuery || u.RawQuery != "" ||
		u.Host != "" && !strings.EqualFold(u.Host, "localhost") {
		return "", false
	}
	if !strings.HasPrefix(u.Path, "/") || strings.HasPrefix(u.Path, "//") ||
		strings.ContainsRune(u.Path, '\\') || strings.ContainsFunc(u.Path, unicode.IsControl) {
		return "", false
	}
	return u.Path, true
}
