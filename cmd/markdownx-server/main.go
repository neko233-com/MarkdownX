// Command markdownx-server shares a folder of Markdown files over HTTP, with
// every accepted save recorded as a Git commit.
//
//	markdownx-server -root ~/notes -addr :8787 -token s3cret
package main

import (
	"encoding/base64"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"time"

	"github.com/neko233/markdownx/internal/server"
)

func main() {
	root := flag.String("root", ".", "工作区目录（Git 仓库）")
	addr := flag.String("addr", ":8787", "监听地址")
	token := flag.String("token", "", "访问令牌；留空则读取 MARKDOWNX_SERVER_TOKEN")
	initRepo := flag.Bool("init", false, "工作区不是 Git 仓库时创建一个")
	encKeyFile := flag.String("encryption-key-file", "", "32 字节 server-side 加密 key（raw 或 base64）")
	flag.Parse()

	if *token == "" {
		*token = os.Getenv("MARKDOWNX_SERVER_TOKEN")
	}
	abs, err := filepath.Abs(*root)
	if err != nil {
		log.Fatalf("无法解析工作区路径：%v", err)
	}
	if info, serr := os.Stat(abs); serr != nil || !info.IsDir() {
		log.Fatalf("%s 不是一个目录", abs)
	}

	if !server.IsRepo(abs) {
		if !*initRepo {
			log.Fatalf("%s 不是 Git 仓库；加 -init 让服务器初始化一个", abs)
		}
		if err := server.InitRepo(abs); err != nil {
			log.Fatalf("初始化仓库失败：%v", err)
		}
		log.Printf("已在 %s 初始化 Git 仓库", abs)
	}

	cfg := server.Config{Root: abs, Token: *token}
	if *encKeyFile != "" {
		key, err := loadKey(*encKeyFile)
		if err != nil {
			log.Fatalf("读取加密 key 失败：%v", err)
		}
		cfg.EncryptionKey = key
		log.Printf("启用磁盘端到端加密（key 文件: %s）", *encKeyFile)
	}

	srv := server.New(cfg)
	if *token == "" {
		log.Print("警告：没有设置访问令牌，局域网内任何人都能读写这个工作区")
	}

	httpSrv := &http.Server{
		Addr:              *addr,
		Handler:           srv.Handler(),
		ReadHeaderTimeout: 10 * time.Second,
	}
	log.Printf("MarkdownX 协作服务：http://localhost%s · 工作区 %s", *addr, abs)
	if err := httpSrv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		log.Fatal(err)
	}
}

// loadKey reads a server-side encryption key from a file. Accepts either
// 32 raw bytes or base64 text (as produced by `mdx-enc genkey`).
func loadKey(path string) ([]byte, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	// Trim trailing whitespace so a base64 key with a final newline still works.
	trimmed := data
	for len(trimmed) > 0 && (trimmed[len(trimmed)-1] == '\n' || trimmed[len(trimmed)-1] == '\r' || trimmed[len(trimmed)-1] == ' ') {
		trimmed = trimmed[:len(trimmed)-1]
	}
	if len(trimmed) == 32 {
		return trimmed, nil
	}
	if b64, err := base64.StdEncoding.DecodeString(string(trimmed)); err == nil && len(b64) == 32 {
		return b64, nil
	}
	return nil, fmt.Errorf("key 文件 %s 必须是 32 字节 raw 或 base64", path)
}
