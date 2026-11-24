// Command markdownx-convert prints a document as Markdown on standard output:
//
//	markdownx-convert report.docx > report.md
//
// It reads the same formats the desktop app imports.
package main

import (
	"fmt"
	"os"

	"github.com/neko233/markdownx/internal/convert"
)

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "用法: markdownx-convert <文件> [输出.md]")
		os.Exit(2)
	}
	in := os.Args[1]
	data, err := os.ReadFile(in)
	if err != nil {
		fmt.Fprintf(os.Stderr, "读取失败: %v\n", err)
		os.Exit(1)
	}
	md, err := convert.Markdown(in, data)
	if err != nil {
		fmt.Fprintf(os.Stderr, "%v\n", err)
		os.Exit(1)
	}
	if len(os.Args) >= 3 {
		if err := os.WriteFile(os.Args[2], []byte(md), 0o644); err != nil {
			fmt.Fprintf(os.Stderr, "写入失败: %v\n", err)
			os.Exit(1)
		}
		return
	}
	fmt.Print(md)
}
