# 团队工作流

MarkdownX 把仓库本身当作工作区：设计、产品、写作和工程文档可以放在同一棵目录树里，
用普通 Markdown 和 Git 协作。

## 工作区

用 **文件 → 打开文件夹…** 打开一个目录。侧栏列出其中的 Markdown 文件，点击切换；
最近打开的文件会记在设置里。文档仍是磁盘上的 `.md` 文件，应用不建立私有数据库。

- 编辑时 500ms 防抖自动保存；关闭前若有未保存修改会询问。
- 外部改动不会被静默覆盖；保存失败会保留编辑内容并提示。
- 打开的文档可以导出为 Markdown、单文件 HTML 或 `.mdxb` 书籍。

## 模板

内置模板在应用里直接可用；**文件 → 存为模板…** 会把当前文档备份到
`os.UserConfigDir()/MarkdownX/templates/`，之后从侧栏“模板”页插入。
模板是普通 Markdown 文件，可以直接用外部编辑器修改。

## 协作服务器

```sh
markdownx-server -root /srv/studio-repo -addr :8787 -token "$(openssl rand -hex 32)" -init
```

浏览器打开 `http://localhost:8787/?token=...`。服务器：

- 列出、读取、保存工作区里的 Markdown；
- 每次接受的保存以当前 Git 分支提交；
- 提供历史、diff、restore；
- 通过 SSE 把其他人的保存推给打开的编辑器；
- 可选 token 鉴权和磁盘端到端加密。

桌面端和浏览器共享同一批文件与 Git 历史，不共享实时编辑会话。

## 分支

- `main`：已审核、可发布的内容。
- `feature/<slug>`：一个功能、一个章节或一轮 sprint。
- `review/<name>`：可选的编辑审阅分支。

服务器不会替你创建或合并分支；用 Git 完成。

## 文件组织建议

```text
studio-repo/
├── design/       # 设计、设定、评审
├── product/      # PRD、研究、路线图
├── docs/         # 手册、会议记录、决策
├── templates/    # 模板
└── assets/       # 图片、附件
```

一个文件一个主题；用链接而不是复制。决策记录写清选择、替代方案和代价。
不要把密钥或 token 写进 Markdown、HTML 或仓库。
