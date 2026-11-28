package app

import (
	"io/fs"
	"os"
	"syscall"
	"time"

	"golang.org/x/sys/unix"
)

func moveWorkspacePath(from, to string) error {
	if err := unix.RenamexNp(from, to, unix.RENAME_EXCL); err != nil {
		return &os.LinkError{Op: "rename", Old: from, New: to, Err: err}
	}
	return nil
}

func workspaceCreationTime(info fs.FileInfo) time.Time {
	if data, ok := info.Sys().(*syscall.Stat_t); ok && data.Birthtimespec.Sec != 0 {
		return time.Unix(data.Birthtimespec.Sec, data.Birthtimespec.Nsec)
	}
	return info.ModTime()
}

func workspaceHidden(info fs.FileInfo) bool {
	data, ok := info.Sys().(*syscall.Stat_t)
	return ok && data.Flags&unix.UF_HIDDEN != 0
}

func workspaceNativeAltPressed() bool { return false }
