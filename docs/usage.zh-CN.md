# Plankton 使用指南

[← README](../README.zh-CN.md) · [English](./usage.md)

## 1. 通过 Homebrew 安装

默认安装路径是项目自有 tap 加 desktop cask：

```bash
brew install --cask flowavelab/tap/plankton
open -a Plankton
plankton --version
```

这是一条 tap 自有 cask 路径，不是 `homebrew-core` formula。这个 cask 会一起安装 `Plankton.app` 和 `plankton` 命令；应用和命令包含在同一个下载包中，无需单独安装 helper formula。

## 2. 通过源码安装并准备本地开发环境

```bash
make install
export PLANKTON_DATABASE_URL="sqlite://$PWD/.plankton/local.db"
mkdir -p .plankton
make check
```

## 3. 启动桌面 UI

```bash
make tauri-dev
```

保持桌面窗口开启。日常使用应以 UI 为中心。

## 4. 在 UI 中选择策略模式

- `人工审批` 是 UI 中专门用于人工审批的策略模式。人工审批发生在桌面 UI 中，不是 CLI 审批流；`plankton run` 也不会再通过命令行参数覆盖这个模式。
- `assisted` 会先向 provider 获取建议，再由桌面 UI 中的人类做最终决定。
- `auto` 会在本地护栏和 provider 建议的基础上自动得到 allow、deny 或 escalate，同时让结果在 UI 和 CLI 中都可见。

## 5. 使用与后端无关的 CLI

**0.2.4 / 协议 6 破坏性变更：**桌面应用与 CLI 必须一起升级，协议 5 及更早版本不能使用新执行 API。原始值 `get`/`request` 及其 access/status API 已禁用，包括既有审批和 Direct 字段。请使用下方 `run` 命令绑定消费者；补充声明或人工批准都不能恢复无消费绑定的原始值接口。既有目录元数据和暴露面配置会保留。

面向 AI 的 CLI 提供五类操作：

```bash
plankton list
plankton search api-token --tag production --field-key token --notes rotate
plankton password add --env API_TOKEN
plankton password add --file ./secrets.yml --key service.token
plankton skill
plankton skill install --agent codex
```

```bash
plankton run secret/api-token --env API_TOKEN \
  --reason "Query api.example.com status without exposing credentials" \
  --requested-by agent -- python3 ./api-client.py --status
```

`run RESOURCE [--env NAME] -- COMMAND ARGS...` 绑定实际消费者，由守护进程采集、审批并执行。不指定 `--env` 时，凭据通过消费者 stdin 注入；指定后则注入该消费者的环境变量。CLI 只传变量名，不接触凭据值，不接受 `NAME=VALUE`。凭据也不得写入命令参数或源码。`--script FILE [--python PATH] -- ARGS...` 保留为 Python 文件快捷形式，默认解释器为 `/usr/bin/python3`，不限制通用命令模式。

守护进程识别脚本文件、受支持的内联代码（Python `-c`、Node `-e`）和 shell `-c` 包装，采集源码与静态消费者，并在审批前绑定可执行程序和文件。环境变量注入、内联代码、heredoc 本身不是违规行为。未知二进制或无法绑定实际消费者的命令会在审批前拒绝，人工批准不能绕过缺失的识别器；这不代表任意 shell 表达式和导入依赖都能完整采集。

Shell 须以非登录、非交互模式运行，`zsh` 必须加 `-f` 禁用用户启动文件。用 `--cwd` 指定工作目录，不在 shell 内 `cd`；脚本路径须为字面量，不能依赖 `~` 或变量选择。允许 `${API_TOKEN}` 这类简单环境引用，拒绝复杂展开，以及对已采集入口文件的 shell 输出重定向。系统运行时启动逻辑、原生工具配置和已安装依赖仍属于可信本地环境，不保证全部源码都已绑定。

程序从 stdin 读取源码时（如 `python3 -`、`node -`、`bash -`），必须使用 `--env` 注入凭据，避免源码与凭据争用 stdin。此时 CLI 会采集输入的源码，包括带引号的 heredoc，交给守护进程识别。要传入业务数据，可用 `--env NAME --input FILE` 或 `--input -`；守护进程区分源码和业务输入，不会把所有 stdin 都标作代码。输入须为 UTF-8、最多 64 KiB，不能包含凭据；拒绝交互终端读取，管道五秒未结束则超时。普通命令不会默认读取调用方 stdin。

`--cwd PATH` 指定消费者目录，命令中的相对路径在该目录解析；虚拟环境请显式传解释器路径。守护进程绑定识别到的执行内容，并在启动前复核文件。此机制不是沙箱，不完全隔离安装依赖、动态 I/O 或同一系统账号的并发文件修改。旧的原始值 `get`/`request` 和 access/status API 仍然禁用，Direct 字段也不例外。

`--timeout` 为消费者执行超时（1–300 秒，默认 60），不含审批与凭据读取时间；外部后端每条命令另有 60 秒期限，每流限 2 MiB。消费者 stdout、stderr 各限 64 KiB，超限会停止执行并丢弃输出。只有 `terminal_log` 和 `llm_context` 同时允许，输出才可包含凭据原值；否则失败且不返回输出。短值可能误匹配普通文本，编码或拆分形式也不能保证识别。文本模式转发消费结果和退出码，JSON/JSONL 返回执行状态与结果，没有原始凭据字段。中断 CLI 不取消守护进程已开始的执行；确认原请求结果后再决定是否重提。

从 1Password 复制指定字段到确认草稿（需安装 `op` CLI，并启用桌面集成或登录）：

```bash
plankton password add \
  --onepassword 'PASSWORD=op://Work/GitHub/password' \
  --onepassword 'USERNAME=op://Work/GitHub/username' \
  --title 'GitHub 凭据'
```

`--onepassword`（别名 `--1password`）可重复使用。可选的 `KEY=` 指定导入后的字段名；
导入同名字段时需分别取名。`--onepassword-account ACCOUNT` 选择来源账号，
`--backend` 和 `--vault` 建议保存位置。此操作复制导入时的值，不建立自动同步关系。
CLI 不输出密码；桌面弹窗允许修改值、暴露面配置和保存位置，用户最终确认后才保存。
任一字段读取失败都不会创建草稿。

LLM 可在导入环境变量、文件或 1Password 时附上可编辑的暴露面配置：

```bash
plankton password add --env API_TOKEN --access-mode protected \
  --network 1 --network-domain api.example.com --process-propagation 1 \
  --exposure-note 'network=仅用于声明的 API 端点'
```

人可以在确认保存前修改导入的密码值和暴露面配置。每个密码输入框右侧都有独立的眼睛按钮；
Direct 字段自动显示。人工修改后的密码不会回传 CLI。

每个集合保存一份默认暴露面配置，字段默认继承；选择“自定义”会复制当前默认配置，之后独立调整。密码管理沿用相同的持久化继承机制。已有的显式单项配置保留为自定义，可手动切回继承。

`list` 和 `search` 只暴露元数据；搜索覆盖名称、别名、备注、Tag、字段
key/label、Section 和 metadata，并支持稳定分页。`run` 始终需要执行准入；Direct 字段可跳过审批决定，但不能跳过准入。`password add` 不会直接写入保险库：它只创建
一次性草稿并打开桌面确认弹窗，由人类检查准确内容后选择 Plankton 或已经显式
开启的外部后端。CLI 的元数据编辑和删除也只提交待人工确认的变更；
CLI 不提供 approve、reject 或绕过确认直接写入密码管理器的命令。

如果这次请求不能自动完成，Plankton 会把流程交给桌面 UI。人工审批、建议查看和审计查看都在 UI 中完成。拒绝或准入失败不会执行消费者。消费者以非零状态退出时，仍可能返回限额内的业务输出，同时保留非零退出码。

如果你当前是在源码仓库里做本地开发，而不是使用 cask 安装，可以把同样的命令换成 `cargo run -p plankton -- ...`。

## 6. 只有在需要 assisted 或 auto 时才配置 provider

`人工审批` 不需要 provider。

OpenAI-compatible：

```bash
export PLANKTON_PROVIDER_KIND=openai_compatible
export PLANKTON_OPENAI_API_KEY=...
export PLANKTON_OPENAI_MODEL=...
```

ACP 支持 Codex、Claude Code、OpenCode、Pi 预设。默认版本策略是 `latest`；
“智能体与模型”页面可以固定精确语义版本，或选择自定义可执行程序。产品默认智能体
仍为 Codex，选择 Pi 只修改当前安装的智能体配置。

Pi 预设使用 ACP Registry 收录的 `pi-acp`，需要 Node.js 22+，以及已安装且位于
`PATH` 的 `pi` CLI（0.81.0 或更新版本），并在 `~/.pi/agent/auth.json` 配置供应商凭据。Plankton 的 ACP 配置不存储 Key，模型和
推理档位使用 Pi 返回的选项。由于该适配器新建会话会替换旧会话，Plankton 对 Pi 禁用
预热并依次处理审批与聊天，避免打断正在进行的审批。

Claude：

```bash
export PLANKTON_PROVIDER_KIND=claude
export PLANKTON_CLAUDE_API_KEY=...
export PLANKTON_CLAUDE_MODEL=...
```

## 7. 密码保险库与可选后端

Plankton 包装固定版本、校验 SHA-256 的 KeePassXC 引擎，本地密码写入 KDBX4。
目录只保存实时字段定位信息，不复制 KDBX 中的值。桌面信息模型为
“保险库 → 分组 → 项 → Section → Field → Tag”。

1Password 与 Bitwarden 都是默认关闭的可选能力。开启连接时会执行真实 CLI
健康/认证检查；只有开启后的后端才会出现在人工确认弹窗。AI 的搜索和读取结果
不会暴露底层 provider、账号、保险库实现、可执行程序或 session。AI 侧厂商命令
策略只允许只读 list/search/get，并拒绝写命令、文件输出和 session token 参数。

“连接”页面还可以选择性配置加密 blob 同步：本地/云盘文件夹、Git、WebDAV
或自定义 HTTP。同步边界只允许完整 KDBX 字节和非敏感 revision/hash 元数据；
本地解锁文件与明文字段永远不会上传。冲突和传输错误会显示在连接状态与诊断页。
