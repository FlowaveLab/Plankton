# Plankton usage guide

[← README](../README.md) · [中文](./usage.zh-CN.md)

## 1. Install with Homebrew

The default install path is the project-owned tap and desktop cask:

```bash
brew install --cask flowavelab/tap/plankton
plankton
```

This is a tap-owned cask, not a `homebrew-core` formula. The cask installs both `Plankton.app` and the `plankton` command. An internal helper formula may still exist inside the tap, but it is not the user-facing entrypoint.

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

- `Human Review` is the UI-only strategy mode for human approval. A human reviews and approves or rejects in the desktop UI. This is not a CLI approval flow, and `plankton get` does not override it with a command-line policy flag.
- `assisted` asks a provider for a suggestion, then keeps the final human decision in the desktop UI.
- `auto` lets local guardrails and the provider produce an automatic allow, deny, or escalate outcome, while keeping the result visible in both UI and CLI.

## 5. Use the provider-neutral CLI

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
set +x
set -o pipefail
plankton get secret/api-token \
  --reason "Use the credential only in the declared consumer process" \
  --requested-by alice |
  downstream-command --token-stdin
```

`downstream-command` is a placeholder: replace it with a consumer that supports stdin and does not echo or log the secret. Use an existing resource ID from `search`. Never run `get` alone in a model-visible terminal.

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
`get` always creates an access request; explicitly configured Direct fields bypass approval; successful text output is only the
approved value. `password add` does not write a vault: it creates a single-use
draft and opens the desktop confirmation dialog, where the human reviews the
exact values and chooses Plankton or an explicitly enabled external backend.
CLI metadata edits and deletions likewise stage changes for human confirmation.
There are no CLI approve, reject, or direct password-manager write commands.

If the request cannot be completed automatically, Plankton hands off to the desktop UI. Human approval, suggestion review, and audit inspection all happen there. Non-success paths keep `stdout` empty and report status or errors separately. When a request is denied and the recorded decision includes a reason or note, Plankton appends that reason to the deny error. When no reason was recorded, the deny output stays concise.

If you are working from a source checkout instead of the cask, run the same commands with `cargo run -p plankton -- ...`.

## 6. Configure a provider only when you need assisted or auto

`Human Review` does not require a provider.

OpenAI-compatible:

```bash
export PLANKTON_PROVIDER_KIND=openai_compatible
export PLANKTON_OPENAI_API_KEY=...
export PLANKTON_OPENAI_MODEL=...
```

ACP supports Codex, Claude Code, and OpenCode presets. The default version mode
is `latest`; the Agents & Models page can pin an exact semantic version or
select a custom executable.

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
only in the human confirmation dialog. AI search and get responses never expose
the backing provider, account, vault implementation, executable, or session.
AI-side vendor command policies allow read-only list/search/get operations and
reject writes, file output, and session-token flags.

The Connections page also configures opt-in encrypted-blob synchronization:
local/cloud folders, Git, WebDAV, or a custom HTTP endpoint. Only complete KDBX
bytes and non-secret revision/hash metadata cross the sync boundary; the local
unlock file and plaintext fields are never synchronized. Conflicts and
transport errors are visible in the connection state and Diagnostics page.
