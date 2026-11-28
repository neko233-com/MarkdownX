package app

import (
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"golang.org/x/sys/windows"
)

func moveWorkspacePath(from, to string) error {
	src, err := windows.UTF16PtrFromString(workspaceWindowsPath(from))
	if err != nil {
		return err
	}
	dst, err := windows.UTF16PtrFromString(workspaceWindowsPath(to))
	if err != nil {
		return err
	}
	// Do not enable replacement or a copy/delete fallback across volumes.
	if err := windows.MoveFileEx(src, dst, 0); err != nil {
		return &os.LinkError{Op: "rename", Old: from, New: to, Err: err}
	}
	return nil
}

func workspaceWindowsPath(path string) string {
	if len(path) < 248 || !filepath.IsAbs(path) || strings.HasPrefix(path, `\\?\`) {
		return path
	}
	if strings.HasPrefix(path, `\\`) {
		return `\\?\UNC\` + path[2:]
	}
	return `\\?\` + path
}

func workspaceCreationTime(info fs.FileInfo) time.Time {
	if data, ok := info.Sys().(*syscall.Win32FileAttributeData); ok && data.CreationTime != (syscall.Filetime{}) {
		return time.Unix(0, data.CreationTime.Nanoseconds())
	}
	return info.ModTime()
}

func workspaceHidden(info fs.FileInfo) bool {
	data, ok := info.Sys().(*syscall.Win32FileAttributeData)
	return ok && data.FileAttributes&(windows.FILE_ATTRIBUTE_HIDDEN|windows.FILE_ATTRIBUTE_SYSTEM) != 0
}

var workspaceAsyncKeyState = windows.NewLazySystemDLL("user32.dll").NewProc("GetAsyncKeyState")

func workspaceNativeAltPressed() bool {
	const vkMenu = 0x12
	value, _, _ := workspaceAsyncKeyState.Call(vkMenu)
	return value&0x8000 != 0
}
