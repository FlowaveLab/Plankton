---
name: secret-access
description: Obtain and use passwords, API keys, tokens, or other credentials through Plankton for an authorized task. Use when a real credential is needed, missing, or requires approval; discover resource metadata, bootstrap Plankton when needed, and run an inspected consumer command with daemon-injected credentials. Do not use for general explanations of passwords or API keys.
---

# Secret Access

## Overview

Use Plankton's provider-neutral CLI to discover secrets, create human-confirmed
password drafts, propose metadata-only catalog changes, and broker an approved
value into the reviewed consumer command executed by the daemon. The CLI returns the
consumer's result, never a raw-value access response. Keep values out of model
context, script source, arguments, and output.

## Bootstrap and first use

When this skill is loaded and the task needs a credential, continue through the
steps below within the user's existing authorization and the host agent's tool
permissions. Do not stop at recommending commands when you can execute them.
This skill does not grant additional installation, account, or approval rights.

1. Check `command -v plankton`. Reuse an installed CLI; do not reinstall it on
   every invocation. If missing, check `uname -sm` and `command -v brew`.
   The packaged app currently supports Apple Silicon macOS. If Homebrew is
   available and installation is authorized, install the app and CLI together:

   ```bash
   brew install --cask flowavelab/tap/plankton
   plankton --version
   ```

   If the machine is unsupported, Homebrew is absent, or the host blocks the
   install, state that concrete blocker. Do not disable Gatekeeper, remove
   quarantine, install unrelated tools, or weaken host permissions to proceed.
   This already-loaded skill does not need to reinstall itself with `npx`.

2. Start the installed desktop app on macOS:

   ```bash
   open -a Plankton
   ```

   Bare `plankton` prints CLI help; it does **not** start the desktop daemon.
   Use a targeted `plankton search <service-or-field-hint>` to check readiness
   and discover metadata. If startup is still in progress, wait briefly and
   retry once. If connection still fails, report the failure; do not print
   `daemon.json`, which contains a local bearer token, or dump runtime logs.

3. Choose the matching resource from metadata. Prefer a targeted search over
   listing the entire catalog. If it is ambiguous, resolve the service/account
   with the user. If missing, import only an explicitly selected source or use
   `password create` so the human can enter the missing values in the app.

4. Request only the resource the task needs with `plankton run`. Pass the actual
   consuming command after `--`, identify its purpose and destination in
   `--reason`, and choose stdin or `--env NAME` credential injection. The daemon
   identifies the consumer and binds its inspectable source before executing it.
   Replace example resource IDs and hostnames with ones verified for this task.
   Never obtain a secret just to test access.

5. Human setup, vault unlocking, credential entry, and human approval still
   happen in the desktop UI. Existing configured policies may approve, deny,
   or escalate access. A denial is final for that request: do not retry with
   weaker policy settings or a different identity. Report only the operation's
   outcome, never the secret. Resume the user's original task after approval.

## Quick reference

- If the resource identifier is unknown, inspect metadata first:

```bash
plankton search api-token
# Use plankton list only when a targeted search is not enough.
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
  --title "Service API credentials" \
  --key CLIENT_ID \
  --key CLIENT_SECRET \
  --network-domain api.example.com \
  --exposure-note 'network=Authenticate only to this service API' \
  --wait
```

  The human enters every value in Plankton and confirms the save. `--wait`
  returns only the saved resource IDs. After it succeeds, request only the
  resource needed by the consumer with `plankton run` below. Creation
  confirmation is not execution approval: every later `run` still receives its
  normal policy decision or human approval.

## Configure the intended exposure when creating credentials

Before submitting `password create`, identify the actual consuming program,
destination hostnames, and whether the authorized task needs storage or logging.
Propose that scope in the same draft so the human can confirm it with the values.
Replace the example hostname with the service's verified hostname. Do not create
a network credential with the default network-denied profile when its known
purpose is API authentication, then repeatedly request exceptions.

- Keep `--access-mode protected`. It allows automatic decisions within the
  configured scope. `direct` skips review and is not a fix for missing evidence.
- Use repeatable `--network-domain HOST` for exact hosts. This sets network level
  1 when `--network` is omitted. Hosts have no scheme, port, or path. Use
  `--network-subdomains HOST` only when the task needs those subdomains; it does
  not include the apex host. Avoid regex or `--network 2` unless the user needs
  that broader scope.
- The other defaults are `--llm-context 0 --local-persistence 0 --terminal-log 0
  --process-propagation 1`: keep values out of model context, disk, and logs;
  allow delivery to the declared local consumer. Propose persistence only when
  the user's task requires it, with an `--exposure-note local_persistence=...`
  explaining the destination and purpose. Notes explain scope; they do not
  override numeric levels or allowlists.
- `password create` applies one default policy to the new group. If fields need
  different scopes, describe that in the draft and let the human adjust each
  field. Do not quietly apply the widest field policy to the whole group.
- For an existing or imported field, propose a metadata-only change:

```bash
plankton password edit-field secret/api-token \
  --access-mode protected --network-domain api.example.com \
  --llm-context 0 --local-persistence 0 --terminal-log 0 --process-propagation 1 \
  --exposure-note 'network=Authenticate to the service API' \
  --reason "Remember the verified API destination for this field"
```

Exposure flags on `edit-field` replace the whole exposure profile, with omitted
options returning to defaults. Inspect the current field policy in Plankton and
preserve its other intended domains, levels, and notes. Reuse `--change-id` when
proposing changes to several fields. `password add` support varies by installed
version; inspect its help before using exposure flags, or set the profile in the
import dialog / propose `edit-field` after import. No secret value is needed.

Policy confirmation can remove repeat human approvals for matching, sufficiently
evidenced requests that pass execution admission. New destinations, policy
violations, or review failures can still require human review. Unsupported
consumers are rejected before review; human approval cannot bypass admission.
A one-time approval does not change the saved policy. Do not retry a denied
request with a weaker profile.

## Execute a bound consumer command

This requires Plankton 0.2.4 / protocol 6 or later with a matching desktop app
and CLI. Upgrade both together within the authorized installation flow.
`get`, its `request` alias and raw-value access/status APIs are disabled, even
for Direct fields and historical approvals: a declaration alone cannot bind a
consumer. Catalog metadata and exposure policies remain available.

Pass the real command and its literal arguments after `--`. With `--env NAME`,
the daemon injects the approved credential into that consumer environment
variable. Supply only a POSIX variable name, never `NAME=VALUE`. Runtime-control
variables such as PATH, BASH_ENV, ENV, PYTHONPATH, NODE_OPTIONS, LD_* and DYLD_*
are not credential destinations. The CLI never receives the credential value.

```bash
plankton run secret/api-token --env API_TOKEN \
  --reason "Query api.example.com status without logging or persisting credentials" \
  --requested-by agent -- python3 ./api-client.py --status
```

The consumer reads `os.environ["API_TOKEN"]` (or the equivalent API in its
language), performs the authorized task and prints only non-secret results.
Without `--env`, the daemon sends the credential to consumer stdin; use that
mode for a consumer that reads its credential there. Keep all credential values
out of command arguments, source, caller input, output and model context.

Named scripts, recognized inline-code forms (such as Python `-c` and Node `-e`),
and shell `-c` wrappers can be inspected. A wrapper, environment injection or
heredoc is not itself a violation. The daemon identifies the runtime, source,
static child consumers and relevant file evidence; it binds what will actually
run at those recognized entrypoints. If a command has no recognizer or its consumer cannot be bound, stop and
report the unsupported part. Human approval cannot bypass failed admission.
This does not promise that every shell expression, native binary or dynamic
import is supported.

Use non-login, non-interactive shells; `zsh` requires `-f` to disable user
startup files. Use `--cwd` instead of shell `cd`, literal script paths instead
of `~` or variable-selected entrypoints, and simple environment references
such as `$API_TOKEN` or `${API_TOKEN}`. Complex shell expansion and output
redirection onto a reviewed entrypoint are refused. System runtime startup,
native-tool configuration, installed imports and dynamically selected behavior
remain trusted dependencies; the command evidence does not freeze all of them.

When the program itself reads code from stdin (for example `python3 -`, `node -`
or `bash -`), use `--env` so credential delivery does not compete for that same
stdin. Plankton captures the caller's code before submitting it for review:

```bash
plankton run secret/api-token --env API_TOKEN \
  --reason "Query the verified API status and return only the HTTP status" \
  -- python3 - <<'PY'
import os
import urllib.request
request = urllib.request.Request(
    "https://api.example.com/status",
    headers={"Authorization": "Bearer " + os.environ["API_TOKEN"]},
)
with urllib.request.urlopen(request) as response:
    print(response.status)
PY
```

Replace example resource IDs and hosts with ones verified for the task. The
quoted heredoc prevents the caller's shell from expanding its contents.
For a consumer that needs business input, use `--input FILE` or `--input -` with
`--env`; the daemon identifies whether that input is code or task data. It is
not automatically labeled as source evidence. Caller input must be UTF-8, at
most 64 KiB, and must not contain credentials. Stdin capture rejects an
interactive terminal and times out after five seconds if the input never ends.
Without `--input`, only recognized stdin-code consumers cause a caller-stdin
read; ordinary commands do not read it implicitly.

`--script FILE [--python PATH] -- ARGS...` remains a Python-file convenience
form; `/usr/bin/python3` is its default interpreter. It is not a restriction on
the command form. `--cwd PATH` sets the consumer directory; relative command
paths resolve there. Use an explicit interpreter path for a virtual environment.
The daemon records the recognized execution and rechecks bound file identities
before launch. Keep these files unchanged while approval is pending.

Each `run` submits once and polls the same execution. Human review opens the
desktop flow when required. Interrupting the CLI does not cancel an execution
already started by the daemon; do not blindly resubmit after a disconnect.
`--timeout` bounds consumer execution to 1–300 seconds (default 60), excluding
review and credential-read time. External credential-backend commands have a
separate 60-second deadline and 2 MiB limit per output stream.

This binds the consumer and credential handoff; it is not a sandbox. Installed
dependencies, dynamic I/O and concurrent changes by another process under the
same OS account are not fully isolated. Missing decision-relevant evidence must
not be replaced with an assertion that a wrapper or filename is safe.

Each consumer output stream is limited to 64 KiB; exceeding it stops execution
and discards output. Filter large responses in the consumer. Literal credential
output is blocked unless both terminal_log and llm_context allow it; short
values may match ordinary text, and this is not an encoding/splitting detector.
Never rely on that guard to make a consumer that echoes credentials acceptable.
Text mode forwards consumer stdout/stderr and exit status. JSON/JSONL return
execution status and output, never a raw credential value field. Denial,
admission failure and execution failure exit nonzero.

## Manage catalog metadata

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

- `password add` never writes a password manager directly. The human confirms the exact draft in the desktop app and chooses Plankton KDBX or an enabled external backend.
- `password create` sends an editable title, key names, destination, and suggested exposure policy. It never accepts password values from CLI arguments or stdin; the human supplies them in one aggregated desktop popup. With `--wait`, successful output contains resource IDs but no values.
- Password management commands never return secret values. They submit metadata-only operations and return a change ID, cumulative diff, state, and optional successor change ID.
- `move-field` and `merge` preserve resource keys while changing password-item membership. `dedupe-field` compares two existing stored values locally and refuses deletion unless they match; neither value is returned.
- A confirmed change ID is immutable. Calls that arrive afterward are accepted under a successor change ID in the same batch instead of being rejected.
- Every `plankton run` is a separate reviewed execution request. Saving a field or its exposure policy does not execute a pending consumer.

## Boundaries

- Management commands may change item IDs, titles, notes, tags, resource keys,
  field membership, retained locators, or delete entries, but their request and
  response schemas cannot carry or return a secret value.
- Supply `--reason` when starting a change. Additional operations using the same
  pending `--change-id` inherit that reason and appear in one cumulative diff.
- Never put credential values into chat, summaries, code, arguments, fixtures,
  logs, screenshots, or terminal transcripts. Use `password create` for missing
  values so the human enters them directly in the app.
- `password add --env NAME` names an existing environment variable to import;
  it is not an instruction to expose that value in request metadata.
- If a task cannot use the supported consumer mode without revealing its
  credential, explain that limitation rather than requesting raw-value access.

## Common mistakes

- Retrying disabled `get`, declaring an unrelated consumer, or using a script that simply prints/encodes its credential.
- Asking the user to paste a missing secret into chat instead of using `password create` for human entry.
- Creating one empty draft per key when the keys belong to the same credential group; use one `password create` call with repeated `--key` options.
- Treating confirmation of `password create` as approval for `run`, or attempting to bypass the normal access request after creation.
- Leaving a known API destination out of the creation draft, or using `direct` / unrestricted network access to suppress repeated approvals.
- Piping an unbound raw `get` result into a consumer, or treating `--consumer-script` metadata as execution binding; use `run` so the daemon inspects and executes the actual command.
- Treating a trusted computer as permission to bypass desktop confirmation.
- Passing a value to a management command. There is deliberately no `--value`
  option on `edit`, `rename-field`, `refresh`, `move-field`, `merge`,
  `dedupe-field`, `delete`, or `change`.
- Describing management as direct password-manager writes. It stages a
  metadata-only daemon change and waits for human confirmation.
