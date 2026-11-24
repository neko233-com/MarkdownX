// Command markdownx is MarkdownX: a Markdown editor drawn by MyGo's native UI,
// with no web view and no C dependencies.
package main

import (
	"log"
	"os"

	"github.com/neko233/markdownx/internal/app"
)

func main() {
	path := ""
	if len(os.Args) > 1 {
		path = os.Args[1]
	}
	if err := app.Run(path); err != nil {
		log.Fatal(err)
	}
}
