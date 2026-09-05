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

- `人工审批` 是 UI 中专门用于人工审批的策略模式。人工审批发生在桌面 UI 中，不是 CLI 审批流；`plankton get` 也不会再通过命令行参数覆盖这个模式。
- `assisted` 会先向 provider 获取建议，再由桌面 UI 中的人类做最终决定。
- `auto` 会在本地护栏和 provider 建议的基础上自动得到 allow、deny 或 escalate，同时让结果在 UI 和 CLI 中都可见。

## 5. 使用与后端无关的 CLI

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
set +x
set -o pipefail
plankton get secret/api-token \
  --reason "Use the credential only in the declared consumer process" \
  --requested-by alice |
  downstream-command --token-stdin
```

`downstream-command` 是占位命令：请替换为支持标准输入、且不会回显或记录密码的实际程序。资源 ID 应来自 `search`。不要在模型可见的终端中单独运行 `get`。

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
key/label、Section 和 metadata，并支持稳定分页。`get` 总是先创建访问请求；显式配置为 Direct 的字段会跳过审批。
成功时 text 输出只有获批的值。`password add` 不会直接写入保险库：它只创建
一次性草稿并打开桌面确认弹窗，由人类检查准确内容后选择 Plankton 或已经显式
开启的外部后端。CLI 的元数据编辑和删除也只提交待人工确认的变更；
CLI 不提供 approve、reject 或绕过确认直接写入密码管理器的命令。

如果这次请求不能自动完成，Plankton 会把流程交给桌面 UI。人工审批、建议查看和审计查看都在 UI 中完成。非成功路径保持 `stdout` 为空，状态或错误会单独输出。若一次 deny 记录里带有原因或备注，Plankton 会把该原因追加到 deny 错误里；如果没有记录原因，则继续保持简洁的 denied 提示。

如果你当前是在源码仓库里做本地开发，而不是使用 cask 安装，可以把同样的命令换成 `cargo run -p plankton -- ...`。

## 6. 只有在需要 assisted 或 auto 时才配置 provider

`人工审批` 不需要 provider。

OpenAI-compatible：

```bash
export PLANKTON_PROVIDER_KIND=openai_compatible
export PLANKTON_OPENAI_API_KEY=...
export PLANKTON_OPENAI_MODEL=...
```

ACP 支持 Codex、Claude Code、OpenCode 预设。默认版本策略是 `latest`；
“智能体与模型”页面可以固定精确语义版本，或选择自定义可执行程序。

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
