package app

import (
	"context"
	"path"
	"sync/atomic"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type workspaceWatch struct {
	root    string
	cancel  context.CancelFunc
	changed atomic.Bool
	stopped atomic.Bool
}

func watchWorkspace(root string, filter workspaceFilter, services ui.Services) *workspaceWatch {
	ctx, cancel := context.WithCancel(context.Background())
	state := &workspaceWatch{root: root, cancel: cancel}
	go func() {
		defer func() {
			state.stopped.Store(true)
			services.Invalidate()
		}()
		rules := filter.rules()
		watcher, err := mygo.WatchFiles(ctx, root, mygo.FileWatchOptions{
			Recursive: true,
			HintsOnly: true,
			Ignore: func(rel string, isDir bool) bool {
				return isDir && rel != "." && rules.pruneDirectory(path.Base(rel), rel)
			},
		})
		if err != nil {
			return
		}
		defer watcher.Close()
		for {
			// The first rescan closes the gap between opening the tree and
			// the platform registering the watch.
			state.changed.Store(true)
			services.Invalidate()
			if _, err := watcher.Next(ctx); err != nil {
				return
			}
		}
	}()
	return state
}

func (a *App) stopWorkspaceWatch() {
	if a.treeWatch != nil {
		a.treeWatch.cancel()
		a.treeWatch = nil
	}
}
