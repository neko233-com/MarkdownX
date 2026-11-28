package app

import "os/exec"

func openWorkspaceTerminal(dir string) error {
	return exec.Command("/usr/bin/open", "-b", "com.apple.Terminal", dir).Run()
}
