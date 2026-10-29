# Operator Runbook

This runbook is the command-level path for validating the complete local
Plankton workflow. Version 0.2.4 / protocol 6 is a breaking migration: validate the matching
desktop app and CLI, and migrate raw `get` consumers to `run` before upgrading.

The accepted primary flow is:

```text
CLI list/search -> CLI run -> desktop review when needed -> daemon-owned consumer execution
```

## Prerequisites

- Rust toolchain available locally
- Node.js and npm available locally
- Tauri system prerequisites installed for the current OS

## Unified commands

Use the repository entrypoints:

```bash
make check
make build
make test
make tauri-dev
```

Optional desktop packaging command:

```bash
make desktop-build
```

## Recommended local setup

Install frontend dependencies once before the first local run:

```bash
make install
```

Use a fixed SQLite path for repeatable manual testing:

```bash
export PLANKTON_DATABASE_URL="sqlite:///abs/path/.plankton/e2e.db"
mkdir -p .plankton
```

This avoids mixing acceptance runs with the default OS-specific application data directory.

## Baseline validation

Before exercising the workflow, run:

```bash
make fmt-check
make check
make build
make test
```

Expected result:

- all commands exit successfully
- `cargo check --workspace` is covered by `make check`
- `cargo build --workspace` and the frontend build are covered by `make build`
- `cargo test --workspace` and frontend tests are covered by `make test`

## Human Review flow

### 1. Start the desktop app

From the repository root:

```bash
make tauri-dev
```

Keep the desktop app running.

### 2. List or search available resource identifiers

From another terminal:

```bash
cargo run -p plankton -- list
```

Expected result:

- the CLI prints resource identifiers and minimal metadata only
- no secret value is printed

If you want to narrow that same view before requesting one resource:

```bash
cargo run -p plankton -- search api-key
```

Expected result:

- the CLI returns a filtered subset of the same identifier view used by `list`
- matching is fuzzy on the resource identifier
- no secret value is printed

### 3. Execute an inspectable consumer

Use a synthetic credential and a named Python fixture that reads stdin, performs
the test task, and prints a fixed non-secret result. Do not test with real values.

```bash
cargo run -p plankton -- run secret/api-key \
  --script ./smoke-consumer.py --python /usr/bin/python3 --cwd . \
  --timeout 60 --reason "Run the local synthetic consumer smoke test" \
  --requested-by test-agent
```

Expected result:

- source, interpreter, arguments and working directory pass daemon admission before review
- the CLI submits once and polls the execution ID; human review dispatches at most once
- approval executes the bound consumer with the credential on stdin or its configured environment variable
- text output contains only consumer stdout/stderr, and exit status matches the consumer
- JSON/JSONL contains execution state and output, with no `value` field
- changed bound files, unknown consumers and raw `get` are rejected
- recognized inline/wrapped commands and stdin source with `--env` are inspected and admitted when evidence is complete
- `--input FILE|-` with `--env` captures bounded task input, classified separately from program source
- transient status failures retry status reads without replaying execution

### 4. Approve or reject in the desktop UI when review is required

In the Tauri app:

- select the request from the pending queue
- review the detail panel and rendered prompt
- choose `Approve` or `Reject`
- optionally provide an audit note

Expected result:

- the request disappears from the pending queue after resolution
- the recent audit trail shows the action

### 5. Verify the final state

Back in the terminal output and desktop UI:

Expected result after approval:

- the terminal prints the consumer result without a credential value
- the desktop request audit shows the submission event and the approval event

Expected result after rejection:

- the terminal keeps `stdout` empty and reports the failure on `stderr`
- if the rejection recorded a reason or note, the deny output includes it; otherwise it stays as a concise denied message
- the desktop request audit shows the submission event and the rejection event

## Evidence checker should capture

- successful exit for `make fmt-check`, `make check`, `make build`, `make test`, and when needed `make desktop-build`
- CLI `list` output showing resource identifiers without secret values
- CLI `search` output showing filtered identifiers without secret values
- successful `run` text output showing only the synthetic consumer result
- `run --output json` showing execution status/output and no raw value field
- disabled `get` and unsupported consumers failing before any credential resolution
- the same request visible in the desktop queue
- desktop approval or rejection action recorded in the UI
- desktop detail and request audit views after the decision

## Current behavior

- manual, assisted, and LLM-automatic policies are active and audited
- ACP supports Codex, Claude Code, OpenCode, and Pi with `latest`, pinned, and
  custom executable profiles
- the AI CLI is limited to metadata-only list/search, reviewed consumer execution, and
  password-draft creation; final writes and approval decisions remain human UI
  operations
- local password writes use the bundled KeePassXC/KDBX4 engine; 1Password and
  Bitwarden are optional, disabled by default, and provider-neutral to AI
  callers
- encrypted KDBX blobs can be synchronized through opt-in local folder, Git,
  WebDAV, or custom HTTP adapters
