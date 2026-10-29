# Plankton usage guide

[← README](../README.md) · [中文](./usage.zh-CN.md)

## 1. Install with Homebrew

The default install path is the project-owned tap and desktop cask:

```bash
brew install --cask flowavelab/tap/plankton
open -a Plankton
plankton --version
```

This is a tap-owned cask, not a `homebrew-core` formula. The cask installs both `Plankton.app` and the `plankton` command. Both are included in the same download; no separate helper formula is required.

## 2. Install from source for local development

```bash
make install
export PLANKTON_DATABASE_URL="sqlite://$PWD/.plankton/local.db"
mkdir -p .plankton
make check
```

## 3. Start the desktop UI

```bash
make tauri-dev
```

Keep the desktop window open. Daily use is centered on the UI.

## 4. Choose the strategy in the UI

- `Human Review` is the UI-only strategy mode for human approval. A human reviews and approves or rejects in the desktop UI. This is not a CLI approval flow, and `plankton run` does not override it with a command-line policy flag.
- `assisted` asks a provider for a suggestion, then keeps the final human decision in the desktop UI.
- `auto` lets local guardrails and the provider produce an automatic allow, deny, or escalate outcome, while keeping the result visible in both UI and CLI.

## 5. Use the provider-neutral CLI

**Breaking change in 0.2.4 / protocol 6:** upgrade the desktop app and CLI together. Protocol 5 and earlier cannot use the new execution API. Raw-value `get`/`request` and their access/status APIs are disabled, including existing approvals and Direct fields. Bind each consumer with the `run` command below; declarations and a human approval cannot restore unbound raw access. Existing catalog metadata and exposure policies are retained.

The AI-facing CLI provides five operation families:

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

`run RESOURCE [--env NAME] -- COMMAND ARGS...` binds the actual consumer for inspection and execution. Without `--env`, the daemon supplies the credential on consumer stdin. With `--env`, it supplies that named consumer environment variable; only the variable name crosses the CLI, never the credential value. Keep values out of command arguments and source. `--script FILE [--python PATH] -- ARGS...` remains a Python-file shortcut whose default interpreter is `/usr/bin/python3`.

The daemon recognizes file entrypoints, supported inline-code forms (Python `-c`, Node `-e`) and shell `-c` wrappers, captures their source and static consumers, and binds executable/file identities before review. Environment injection, inline code and heredocs are not inherently violations. An unknown binary or a command whose consumer cannot be bound is rejected before review; human approval cannot bypass a missing recognizer. Supported forms are intentionally explicit, not a claim that every shell expression or imported dependency is captured.

Shells must be non-login and non-interactive; `zsh` requires `-f` to disable user startup files. Use `--cwd` instead of shell `cd`, literal script paths instead of `~` or variable-selected entrypoints, and simple environment references such as `${API_TOKEN}`. Complex expansion and shell output redirection onto a collected entrypoint are refused. System runtime startup, native-tool configuration and installed dependencies remain part of the trusted local environment; their sources are not fully bound by this mechanism.

For stdin program source (`python3 -`, `node -`, `bash -`), use `--env` so the program code and credential do not compete for stdin. The CLI then captures the supplied code, including a quoted heredoc, for daemon inspection. To send business input, use `--env NAME --input FILE` or `--input -`; the daemon distinguishes code from task data. This input must be UTF-8 and at most 64 KiB, with no embedded credentials. Interactive stdin is refused and an open pipe times out after five seconds. Ordinary commands never read caller stdin implicitly.

Use `--cwd PATH` for the consumer directory; relative command paths resolve there. For a virtual environment, pass its interpreter path. The daemon binds the recognized execution and rechecks files before launch. This is not a sandbox: installed dependencies, dynamic I/O and concurrent same-account file changes are not fully isolated. The legacy raw-value `get`/`request` and access/status APIs remain disabled, including Direct fields.

`--timeout` bounds consumer execution to 1–300 seconds (default 60), excluding review and credential-read time. External backend commands have a separate 60-second deadline and 2 MiB per-stream limit. Consumer stdout/stderr are each limited to 64 KiB; excess output stops execution and is discarded. Literal credential output requires both `terminal_log` and `llm_context` permission; otherwise the request fails without output. Short values can match ordinary text, and encoded or split representations are not comprehensively detected. Text mode forwards consumer output and exit status; JSON/JSONL return execution status and output, not a raw credential field. Interrupting CLI does not cancel execution already started by the daemon; confirm the original outcome before resubmitting.

To copy selected fields from 1Password into a confirmation draft (requires the
`op` CLI with desktop integration enabled or an authenticated session):

```bash
plankton password add \
  --onepassword 'PASSWORD=op://Work/GitHub/password' \
  --onepassword 'USERNAME=op://Work/GitHub/username' \
  --title 'GitHub credentials'
```

`--onepassword` (alias `--1password`) is repeatable. `KEY=` is optional and
renames the destination field; use distinct keys when importing fields with the
same name. `--onepassword-account ACCOUNT` selects the source account, while
`--backend` and `--vault` suggest the destination. This copies the selected
values at import time; it does not establish automatic synchronization. The
CLI never prints the values. The desktop opens an editable draft, and saving
requires the human's final confirmation. A read failure creates no draft.

Agents can include an editable exposure profile with any import source:

```bash
plankton password add --env API_TOKEN --access-mode protected \
  --network 1 --network-domain api.example.com --process-propagation 1 \
  --exposure-note 'network=Only use the declared API endpoint'
```

The human can edit imported values and the suggested exposure profile before
confirming the write. Each password field has its own eye button; Direct fields
are shown automatically. Edited values are not returned to the CLI.

Each item stores a collection default. Fields inherit it unless explicitly customized; choosing Custom copies the current default. The password manager uses the same persisted inheritance rules. Existing explicit field policies remain custom until the human switches them to inheritance.

`list` and `search` expose metadata only. Search covers names, aliases, notes,
tags, field keys and labels, sections, and metadata, with stable pagination.
`run` always passes execution admission; Direct fields may skip the review decision but cannot skip admission. `password add` does not write a vault: it creates a single-use
draft and opens the desktop confirmation dialog, where the human reviews the
exact values and chooses Plankton or an explicitly enabled external backend.
CLI metadata edits and deletions likewise stage changes for human confirmation.
There are no CLI approve, reject, or direct password-manager write commands.

If the request cannot be completed automatically, Plankton hands off to the desktop UI. Human approval, suggestion review, and audit inspection all happen there. Denial and admission failures do not execute the consumer. A consumer that exits nonzero can still return its bounded task output, with its nonzero exit code.

If you are working from a source checkout instead of the cask, run the same commands with `cargo run -p plankton -- ...`.

## 6. Configure a provider only when you need assisted or auto

`Human Review` does not require a provider.

OpenAI-compatible:

```bash
export PLANKTON_PROVIDER_KIND=openai_compatible
export PLANKTON_OPENAI_API_KEY=...
export PLANKTON_OPENAI_MODEL=...
```

ACP supports Codex, Claude Code, OpenCode, and Pi presets. The default version mode
is `latest`; the Agents & Models page can pin an exact semantic version or
select a custom executable. The product default agent remains Codex. Selecting
Pi changes only that installation’s configured agent.

The Pi preset runs the ACP Registry adapter `pi-acp` and requires Node.js 22+
and an installed `pi` CLI (0.81.0 or newer) available on `PATH`, with its provider
credentials configured separately in
`~/.pi/agent/auth.json`. Plankton keeps credentials out of its ACP profile;
select the model and thinking level from the options reported by Pi.
Pi sessions run sequentially because its adapter replaces the previous session
when creating a new one. Plankton disables prewarming for this adapter and keeps
Codex/Claude Code session behavior unchanged.

Claude:

```bash
export PLANKTON_PROVIDER_KIND=claude
export PLANKTON_CLAUDE_API_KEY=...
export PLANKTON_CLAUDE_MODEL=...
```

## 7. Password vaults and optional backends

Plankton wraps a pinned, checksum-verified KeePassXC engine and stores local
passwords in KDBX4. The catalog stores live field locators rather than copying
KDBX values. The desktop uses a Vault → Group → Item → Section → Field → Tag
information model.

1Password and Bitwarden are optional and off by default. Enabling a connection
performs a real CLI health/authentication check. Enabled backends may be chosen
only in the human confirmation dialog. AI search and execution responses never expose
the backing provider, account, vault implementation, executable, or session.
AI-side vendor command policies allow read-only list/search/get operations and
reject writes, file output, and session-token flags.

The Connections page also configures opt-in encrypted-blob synchronization:
local/cloud folders, Git, WebDAV, or a custom HTTP endpoint. Only complete KDBX
bytes and non-secret revision/hash metadata cross the sync boundary; the local
unlock file and plaintext fields are never synchronized. Conflicts and
transport errors are visible in the connection state and Diagnostics page.
