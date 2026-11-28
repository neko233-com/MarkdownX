package app

import (
	"os/exec"
	"path/filepath"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

func workspaceTerminalCommand(dir string) (*exec.Cmd, error) {
	if terminal, err := exec.LookPath("wt.exe"); err == nil {
		// Use the process's directory for -d . so folder names remain data
		// even when they contain Windows Terminal command separators.
		cmd := exec.Command(terminal, "new-tab", "--startingDirectory", ".")
		cmd.Dir = dir
		return cmd, nil
	}
	var cmd *exec.Cmd
	if pwsh, err := exec.LookPath("pwsh.exe"); err == nil {
		cmd = exec.Command(pwsh, "-NoLogo")
	} else {
		system, err := windows.GetSystemDirectory()
		if err != nil {
			return nil, err
		}
		cmd = exec.Command(filepath.Join(system, "cmd.exe"), "/D")
	}
	cmd.Dir = dir
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NEW_CONSOLE}
	return cmd, nil
}

func openWorkspaceTerminal(dir string) error {
	cmd, err := workspaceTerminalCommand(dir)
	if err != nil {
		return err
	}
	exe, err := windows.UTF16PtrFromString(cmd.Path)
	if err != nil {
		return err
	}
	args, err := windows.UTF16PtrFromString(windows.ComposeCommandLine(cmd.Args))
	if err != nil {
		return err
	}
	cwd, err := windows.UTF16PtrFromString(cmd.Dir)
	if err != nil {
		return err
	}
	var flags uint32
	if cmd.SysProcAttr != nil {
		flags = cmd.SysProcAttr.CreationFlags
	}
	// A new console supplies its own keyboard and screen. os/exec supplies
	// redirected standard handles when the app has no console of its own.
	var startup windows.StartupInfo
	startup.Cb = uint32(unsafe.Sizeof(startup))
	var child windows.ProcessInformation
	if err := windows.CreateProcess(exe, args, nil, nil, false, flags, nil, cwd, &startup, &child); err != nil {
		return err
	}
	_ = windows.CloseHandle(child.Thread)
	_ = windows.CloseHandle(child.Process)
	return nil
}
