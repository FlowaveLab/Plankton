---
name: secret-access
description: Discover credentials and execute an authorized consumer through Plankton without exposing values. Use when a task needs credentials or credential catalog changes.
---

# Secret Access

## Overview

Use Plankton's provider-neutral CLI to discover secrets, create human-confirmed
password drafts, propose metadata-only catalog changes, and broker an approved
value into the reviewed consumer command executed by the daemon. The CLI returns the
consumer's result, never a raw-value access response. Keep values out of model
context, script source, arguments, and output.

## Use an existing credential

1. Reuse the installed CLI. If absent or unreachable, read
   [setup](references/setup.md). Bare `plankton` prints help; `open -a Plankton`
   starts the macOS desktop service.
2. Find the resource with `plankton search <service-or-field>`. Results contain
   metadata. Resolve an ambiguous account or service before choosing a field.
3. Prepare the actual consumer command. It reads credentials from the named
   environment variables or stdin and returns only the authorized task result.
4. Submit one `run`, binding every credential needed by that same consumer.
   Wait for that execution; continue the original task when it succeeds.

```bash
plankton search service
plankton run RESOURCE_A --env CLIENT_ID \
  --bind RESOURCE_B=CLIENT_SECRET \
  --reason "Authenticate the authorized request to api.example.com" \
  -- python3 ./client.py
```

Replace resource IDs and destinations with verified task-specific values.
Omit `--bind` for one credential. `--bind RESOURCE=ENV` adds another credential
injection to this same command; `ENV` is a variable name, never its value.
Each field is checked against its own policy. The consumer starts once, only
after all required fields pass. This request does not cover future independent
Plankton calls or authorize unknown dynamic credential use.

Use `--env` for environment delivery; without it, credential delivery uses
consumer stdin. When stdin contains program code or business input, use `--env`
and the appropriate input form in [execution](references/execution.md).
`--cwd` sets the consumer directory; `--timeout` limits consumer runtime, not
approval waiting. Keep bound files unchanged while approval is pending.

## Follow the returned state

Plankton may approve, deny, or wait for a desktop decision. Surface a human
handoff while continuing to wait on the same execution. A denial is final for
that request; do not weaken policy or change identity to retry. Disconnecting
or interrupting the CLI does not cancel an already-started consumer. Check its
outcome before resubmitting so the operation is not executed twice.

Credentials stay out of chat, source, command arguments and output. Raw `get`
and raw-value APIs are disabled. `run` binds the actual consumer; it is not a
sandbox and does not freeze dynamic dependencies. Unsupported consumer modes
cannot be enabled by human approval.

## Read only the relevant details

Outside an installed skill directory, use `plankton skill show TOPIC` to read
any reference below (`setup`, `credentials`, `execution`, `catalog`, `boundaries`).

- Missing CLI, desktop startup or compatibility: [setup](references/setup.md).
- Missing credentials, selected imports, or exposure-policy proposals:
  [credentials](references/credentials.md). Open one `password create` draft
  for related keys; the human supplies values in the app. Creation approval
  does not approve execution. Include known destinations in the same draft.
- Inline code, heredocs, pipelines, stdin/data, wrappers, limits or execution
  failures: [execution](references/execution.md).
- Rename, move, merge, refresh, deduplicate, delete or other catalog changes:
  [catalog](references/catalog.md). These stage changes for confirmation;
  reuse `--change-id` to aggregate a pending change.
- Credential handling or authorization edge cases:
  [boundaries](references/boundaries.md).

Use existing task authorization and host permissions. The skill grants no
additional installation, account, credential or approval rights. Do not fetch
a credential merely to test access or ask for its value in chat.
