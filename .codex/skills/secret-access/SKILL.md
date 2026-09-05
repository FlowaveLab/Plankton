---
name: secret-access
description: Use when a task requires a password, API key, token, credential, secret, or other sensitive value, especially when the credential is missing, its provider-neutral resource identifier is unknown, or access needs human approval.
---

# Secret Access

## Overview

Use Plankton's provider-neutral CLI to discover secrets, create human-confirmed
password drafts, propose metadata-only catalog changes, and broker an approved
value directly into the process that needs it. Never expose a returned value to
the model.

## Install

```bash
brew install --cask flowavelab/tap/plankton
plankton
```

## Quick reference

- If the resource identifier is unknown, inspect metadata first:

```bash
plankton list
plankton search api-token
```

- If the resource does not exist, create a draft from an explicitly selected
  environment variable, file field, or 1Password field reference:

```bash
plankton password add --env API_TOKEN
plankton password add --onepassword 'TOKEN=op://Work/Service/password'
plankton password add --file .env --key API_TOKEN --title "Production API"
```

For 1Password, install `op` and enable desktop integration or sign in first.
Repeat `--onepassword` for each selected `op://VAULT/ITEM/FIELD` reference;
optional `KEY=` aliases must be unique. `--onepassword-account` selects the
source account; `--backend` and `--vault` suggest the destination. This copies
selected values into a draft, not a live synchronization link. The human can
edit the draft and must confirm before saving. No values are printed, and any
read failure creates no draft. Do not use shell substitution with `op read` or
paste secret values into arguments or chat.

Agents may attach a suggested exposure profile to `password add` for env,
file, or 1Password imports using the same flags as `password create`: `--access-mode`,
`--breach-action`, `--llm-context`, `--network`, `--local-persistence`,
`--terminal-log`, `--process-propagation`, `--network-domain`,
`--network-subdomains`, `--network-regex`, and repeatable `--exposure-note`.
For example: `plankton password add --env API_TOKEN --network 1 --network-domain api.example.com`.
These are suggestions: the human can edit both imported password values and
exposure profiles before confirming the write. Human edits are never returned
to the CLI. Exposure flags set the collection default; fields inherit it unless
an explicit field override is supplied. Choosing Custom copies the current
default, and later collection edits leave custom fields unchanged. Omitted
exposure flags retain the protected defaults.


- If an entire credential group is missing, open one aggregated desktop form
  with the title and every required key pre-filled, but no password values:

```bash
plankton password create \
  --title "Example service credentials" \
  --key CLIENT_ID \
  --key CLIENT_SECRET \
  --wait
```

  The human enters every value in Plankton and confirms the save. `--wait`
  returns only the saved resource IDs. After it succeeds, request only the
  resource needed by the downstream process with the ordinary `plankton get`
  flow shown below. Creation confirmation is not read approval: every later
  `get` must still receive its normal policy decision or human approval.

- To propose catalog management changes, use the metadata-only commands. Every
  new change requires an audit reason and opens desktop confirmation:

```bash
plankton password edit production-api \
  --title "Production API" \
  --reason "Normalize the catalog title"

plankton password rename-field secret/old-key --to secret/new-key \
  --reason "Move callers to the canonical resource key"

plankton password refresh production-api \
  --reason "Refresh the retained upstream locator"

plankton password move-field secret/service-username \
  --to-item service-credentials \
  --title "Service credentials" \
  --reason "Split service fields into a dedicated item"

plankton password merge legacy-service \
  --into service-credentials \
  --reason "Merge legacy fields into the canonical item"

plankton password dedupe-field secret/duplicate-token \
  --keep secret/canonical-token \
  --reason "Remove a locally verified duplicate"

plankton password delete retired-api \
  --reason "Remove a retired catalog entry"
```

- Reuse `--change-id` to aggregate several calls into one cumulative diff. The
  default `--commit async` returns after staging; `--commit sync` waits for the
  confirmed version to finish. Query either mode explicitly:

```bash
plankton password change CHANGE_ID
plankton password change CHANGE_ID --wait --timeout 300
```

- If the resource identifier is known, pass its approved value directly to a
  downstream command. The downstream process must not echo or log the value:

```bash
set +x
set -o pipefail
plankton get secret/api-token \
  --reason "为完成当前开发联调，需要临时获取目标服务凭证，仅通过标准输入交给下游进程，不写入文件或日志" \
  --requested-by alice |
  downstream-command --token-stdin
```

- `password add` never writes a password manager directly. The human confirms the exact draft in the desktop app and chooses Plankton KDBX or an enabled external backend.
- `password create` sends only an editable title, key names, and destination suggestion. It never accepts password values from CLI arguments or stdin; the human supplies them in one aggregated desktop popup. With `--wait`, successful output contains resource IDs but no values.
- Password management commands never return secret values. They submit metadata-only operations and return a change ID, cumulative diff, state, and optional successor change ID.
- `move-field` and `merge` preserve resource keys while changing password-item membership. `dedupe-field` compares two existing stored values locally and refuses deletion unless they match; neither value is returned.
- A confirmed change ID is immutable. Calls that arrive afterward are accepted under a successor change ID in the same batch instead of being rejected.
- Every `plankton get` is an explicit access request. Approval may happen outside the CLI flow before the value is returned.
- Successful text output is the resolved raw value, so never run `get` as a standalone command through a model-visible shell.
- For AI-facing access, use the default text output only. Do not use `--output json` or `--output jsonl` with `get`; both serialize the resolved value.

## Boundaries

- Treat any value returned by `plankton get` as use-only sensitive material.
- Management commands may change item IDs, titles, notes, tags, resource keys,
  field membership, retained locators, or delete entries, but their request and
  response schemas cannot carry or return a secret value.
- Supply `--reason` when starting a change. Additional operations using the same
  pending `--change-id` inherit that reason and appear in one cumulative diff.
- Do not let the model itself see the returned value. Do not run `plankton get` in a way that captures the secret into model-visible command output, reasoning, summaries, or copied snippets.
- Do not paste the returned value into the chat, summaries, code comments, logs, patches, screenshots, fixtures, tests, markdown examples, or terminal transcripts quoted back to the user.
- Do not persist a value returned by AI-facing `plankton get` unless the user's task explicitly requires that destination. Human-confirmed `password add` persistence is a separate, authorized desktop workflow.
- Do not restate, paraphrase, quote, or otherwise reveal the value back to the model. Broker it only inside the same shell invocation that runs the downstream consumer, without inspecting it.
- Prefer piping, environment injection, or direct process handoff over temporary files.
- `password add --env NAME` names the environment variable to import. In contrast, `get --env KEY=VALUE` and `get --metadata KEY=VALUE` send request metadata that may be recorded; never put a secret in either argument.
- Disable shell tracing (`set +x`) before any handoff and verify the downstream command does not print its arguments, environment, or stdin.
- If the next step would require showing the secret to the model, stop and explain that Plankton values must be consumed without being disclosed back into the conversation.

## Common mistakes

- Running `plankton get` alone and allowing its stdout to enter the agent transcript.
- Asking the user to paste a missing secret into chat instead of creating a `password add` draft.
- Creating one empty draft per key when the keys belong to the same credential group; use one `password create` call with repeated `--key` options.
- Treating confirmation of `password create` as approval for `get`, or attempting to bypass the normal access request after creation.
- Treating a trusted computer as permission to bypass desktop confirmation.
- Passing a value to a management command. There is deliberately no `--value`
  option on `edit`, `rename-field`, `refresh`, `move-field`, `merge`,
  `dedupe-field`, `delete`, or `change`.
- Describing management as direct password-manager writes. It stages a
  metadata-only daemon change and waits for human confirmation.
