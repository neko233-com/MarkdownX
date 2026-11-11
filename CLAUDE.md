# 开发约定（给 AI 助手和人）

先读 README.md 和 docs/ARCHITECTURE.md。本文件与 AGENTS.md 内容相同，修改时两份一起改。

## 当前重点：动态书的编辑、阅读与分发

代码、内置指南、文档和资源都只在这一份里维护。`.mdxb` 是桌面端与浏览器阅读器的共享格式；界面细节以代码里的 `Theme` token 和现有实现为准，和代码对不上时，以代码为准。

## 产品原则

- 保持现有元素、顺序、行为、快捷键和文案；视觉使用现有 Theme 语义 token。
- 唯一的结构性偏离：侧栏沉在窗口底色上（无面板、无边框、无阴影），笔记是贴齐窗口上/右/下边的白色平面（无 gutter、圆角、边框、阴影）。

## 代码

- **不跑 rustfmt。** 代码是刻意的紧凑风格，新代码照周围代码的风格写，注释密度也一样。
- **文档引擎在 `crates/core`（`fdl_core`），不依赖 GPUI。** 模型、格式、编辑内核、文档库、导入和 Markdown 导出放这里，只能引用 core 里的模块；界面代码放根 crate。见 docs/ARCHITECTURE.md。
- **改文档结构只经过 `editor::Core`。** 顺序是：`checkpoint` → Core 方法 → `Editor::changed()`。新增结构性操作时，同时在 `fuzz_tests.rs` 的 `step` 里加一种随机操作。
- 能有子级的只有 `BlockKind::nests()` 的 block 和容器（引用、高亮块、分栏、列）。代码块里没有样式和行内对象。
- **DocxXML 写出的格式要稳定。** 改了读写逻辑，`crates/core/tests/fixtures/all-blocks.xml` 必须仍能原样往返。新增 block 类型时，把它的规范写法加进这个 fixture。
- **颜色用 `Theme::*` 语义 token**，其次是 `theme::craft` 色阶（`c(craft::N300)`）。只有实测值才写十六进制并注明出处。阴影用 `theme::shadow_*`，菜单外壳用 `ui::popover` / `ui::menu_row`。
- 改了 `vendor/` 的代码，同步更新 vendor/PATCHES.md。
- 改了文档库目录布局、会让旧版本误读时，把 `store::FORMAT` 加一。Meta 的新字段一律 `#[serde(default)]`。

## 并行与开发效率

- 默认使用本机可用的多个 CPU 核心，以缩短开发、构建、测试和打包时间；使用正常进程优先级，不主动限制进程到单核或把测试线程固定为 1。只有用户另行要求或实际资源限制需要时才降低并发。
- Cargo 构建和测试优先使用默认并行度或按可用核心数设置 `--jobs`；可并发的独立检查、测试和打包步骤在输出与数据互不冲突时并行执行。共享 `target/`、安装状态或测试数据的步骤避免互相争锁、覆盖或干扰。
- Windows 上先确保当前进程使用完整可用的 MSVC 和 Windows SDK 环境；按需加载开发工具链，不修改系统级配置，也不因此限制并发。
- 保留 `target/` 等构建缓存，优先增量构建和必要的针对性验证；已通过且未受后续修改影响的测试不重复运行。提速不能省略项目要求的验证、数据保护、MSI 构建和本机安装。

## 提交前

```sh
scripts/check.sh        # cargo test --release + clippy --all-targets 零警告
```

改了编辑内核时，多跑一些随机种子：`FDL_FUZZ_SEEDS=2000 cargo test --release random_edits`。

只移动代码、不改外观的重构，用 `tools/harness/compare.py` 对比改动前后的截图，必须完全一致（见 tools/harness/README.md）。

想每次提交自动检查：`git config core.hooksPath scripts/hooks`。

发布版本时，把两个 `Cargo.toml` 的版本号一起更新。未经用户明确要求，不创建提交。

## 每次完成后的本机 MSI 更新

- 每次本项目任务的开发、文档和必要验证全部完成后，都必须先清理测试产物，再构建最新 release、打包当前版本的 Windows MSI，并在本机安装或升级到该最新版本。
- 安装前让正在运行的 MarkdownX 正常自动保存并退出；本项目协作服务如占用程序文件，确认没有活动会话后正常停止，并在安装后恢复。安装后核对 MSI 版本和已安装程序，必要时重启应用。遇到保存失败或安装阻塞时保留现场并明确报告，不强制结束进程，不以仅完成打包代替本机安装。
- 发布版本时同步项目与 MSI 版本；每次构建生成新的 ProductCode，保持 UpgradeCode 不变以支持升级。保留当前 MSI，按清理规则移除旧版本 MSI 和一次性日志。

## 用户数据（绝对不要碰）

- 产品名 MarkdownX，二进制 `markdownx`，默认文档库 `~/Documents/MarkdownX`。那是使用者的真实数据：不要提交进仓库，不要删改里面的文件。别的应用的进程和数据一律不碰，结束进程只能匹配本仓库自己编译出来的二进制。
- 本地预览版是 `dist-preview/markdownx.app`，用 `scripts/preview.sh` 启动或更新，用的也是默认文档库。应用首次启动时，由 `src/guides.rs` 在空文档库中初始化编入二进制的「开始指南」，不覆盖已有文档库。
- `shared-mock/library` 由 `python3 shared-mock/guides/build.py <空目录>` 从 `shared-mock/guides/src/*.xml` 生成：改了指南就重新生成，并用 `FDL_ROUNDTRIP_DIR=<目录>/docs cargo test --release library_round_trips -- --ignored` 确认能原样往返。指南只写已经实现的功能，标题不带 emoji 图标。
- 指南里的截图用 `shared-mock/guides/shots/shots.sh` 重拍（窗口会在屏幕中间浮一分钟左右，不抢焦点）；改了指南里展示的界面后记得重拍。
- 测试用的文档库一律放在临时目录，用 `MARKDOWNX_HOME` 指定。

## 测试前清理

- 每次跑测试或打包前先运行 `python scripts/clean_test_artifacts.py`；`scripts/check.py` 会自动执行。
- 只清理仓库内的固定生成物：`.dsh-*`、`test-results`、`playwright-report`、`.pytest_cache`、`__pycache__`、`dist/*.log`、`dist/*.wixpdb`、`dist/*.wixobj` 和旧版本 MSI。
- 绝不删除 `~/Documents/MarkdownX`、`MARKDOWNX_HOME` 指向的用户库、已安装程序、当前 MSI、`target/` 缓存或 `.git`。
- 测试截图、日志和临时库只放在 `.dsh-*`（已 ignore）或系统临时目录，测试结束前清掉；不要留下未 ignore 的报告文件。
- 新增生成物时，同时更新 `.gitignore` 和 `scripts/clean_test_artifacts.py`。
- 发布前保留当前版本 MSI，删除旧 MSI、安装日志和一次性的测试报告。

## UI 脚本（tools/harness）

规则见 tools/harness/README.md，要点：

- 不用真实系统剪贴板，`key cmd-v` 只在 `clip` 之后用。
- 不用 `activate` 抢焦点。
- 不点这些：
  - 设置 › 文档库的「在访达中显示」；
  - 链接和「网页搜索」；
  - 打印；
  - 快速查看。
- 不用 `open` 命令。
- 选文件用 `pick`。

## 环境注意

- shell 是 zsh：没有匹配的通配符会直接报错，`--include=*.rs` 这类参数要加引号。
- `grep` 实际是 ugrep。
- macOS 的 `sed -i` 要写成 `sed -i ''`。
- 没有 `timeout` 命令。
