// Command mdx-enc encrypts and decrypts MarkdownX pages.
//
//	mdx-enc encrypt doc.md -p secret
//	mdx-enc decrypt doc.md -p secret
//	mdx-enc genkey                    # 写一个随机 32 字节 key 到 stdout (base64)
package main

import (
	"encoding/base64"
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/neko233/markdownx/internal/rage"
)

func usage() {
	fmt.Fprintf(os.Stderr, `MarkdownX 加密工具

用法:
  mdx-enc encrypt <file> -p password [-o out]
  mdx-enc decrypt <file> -p password [-o out]
  mdx-enc genkey [-o keyfile]

示例:
  mdx-enc encrypt notes.md -p s3cret
  mdx-enc decrypt notes.md -o plain.md -p s3cret
  mdx-enc genkey -o server.key
`)
}

func main() {
	if len(os.Args) < 2 {
		usage()
		os.Exit(1)
	}
	sub := os.Args[1]
	args := os.Args[2:]

	switch sub {
	case "encrypt":
		cmdEncrypt(args)
	case "decrypt":
		cmdDecrypt(args)
	case "genkey":
		cmdGenkey(args)
	case "-h", "--help", "help":
		usage()
	default:
		fmt.Fprintf(os.Stderr, "未知子命令: %s\n\n", sub)
		usage()
		os.Exit(1)
	}
}

// reorderFlags moves every -flag value pair to the front of the list so
// Go's flag package (which stops at the first positional arg) sees them.
func reorderFlags(args []string) []string {
	var flags, pos []string
	for i := 0; i < len(args); i++ {
		a := args[i]
		if strings.HasPrefix(a, "-") && len(a) > 1 {
			flags = append(flags, a)
			// -f=value form has value inline; -f value needs one more arg.
			if !strings.Contains(a, "=") && i+1 < len(args) && !strings.HasPrefix(args[i+1], "-") {
				flags = append(flags, args[i+1])
				i++
			}
		} else {
			pos = append(pos, a)
		}
	}
	return append(flags, pos...)
}

func cmdEncrypt(args []string) {
	fs := flag.NewFlagSet("encrypt", flag.ExitOnError)
	password := fs.String("p", "", "密码")
	out := fs.String("o", "", "输出路径（默认覆盖原文件）")
	fs.Parse(reorderFlags(args))
	if *password == "" || fs.NArg() < 1 {
		fmt.Fprintln(os.Stderr, "需要一个文件路径和 -p 密码")
		os.Exit(1)
	}
	src := fs.Arg(0)
	data, err := os.ReadFile(src)
	if err != nil {
		fatal(err)
	}
	if rage.IsRage(data) {
		fatalf("%s 已经是加密格式", src)
	}
	enc, err := rage.EncryptPage(data, *password)
	if err != nil {
		fatal(err)
	}
	dst := *out
	if dst == "" {
		dst = src
	}
	if err := os.WriteFile(dst, enc, 0o644); err != nil {
		fatal(err)
	}
	fmt.Printf("已加密 → %s\n", dst)
}

func cmdDecrypt(args []string) {
	fs := flag.NewFlagSet("decrypt", flag.ExitOnError)
	password := fs.String("p", "", "密码")
	out := fs.String("o", "", "输出路径（默认覆盖原文件）")
	fs.Parse(reorderFlags(args))
	if *password == "" || fs.NArg() < 1 {
		fmt.Fprintln(os.Stderr, "需要一个文件路径和 -p 密码")
		os.Exit(1)
	}
	src := fs.Arg(0)
	data, err := os.ReadFile(src)
	if err != nil {
		fatal(err)
	}
	if !rage.IsRage(data) {
		fatalf("%s 不是加密格式", src)
	}
	plain, err := rage.DecryptPage(data, *password)
	if err != nil {
		fatal(err)
	}
	dst := *out
	if dst == "" {
		dst = src
	}
	if err := os.WriteFile(dst, plain, 0o644); err != nil {
		fatal(err)
	}
	fmt.Printf("已解密 → %s\n", dst)
}

func cmdGenkey(args []string) {
	fs := flag.NewFlagSet("genkey", flag.ExitOnError)
	out := fs.String("o", "", "输出文件（默认 stdout 打印 base64）")
	fs.Parse(reorderFlags(args))
	k, err := rage.GenerateKey()
	if err != nil {
		fatal(err)
	}
	if *out != "" {
		if err := os.WriteFile(*out, k, 0o600); err != nil {
			fatal(err)
		}
		fmt.Printf("已写入 %s  (%d 字节)\n", *out, len(k))
		return
	}
	fmt.Println(base64.StdEncoding.EncodeToString(k))
}

func fatal(err error) {
	fmt.Fprintln(os.Stderr, "错误:", err)
	os.Exit(1)
}

func fatalf(format string, args ...any) {
	fmt.Fprintf(os.Stderr, "错误: "+format+"\n", args...)
	os.Exit(1)
}
