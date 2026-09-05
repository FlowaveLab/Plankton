# Plankton Runtime Correctness Design

**Date:** 2026-07-30

## Goal

Fix the installed-runtime failures found during the Plankton 0.2.0 smoke test:

- repeated field keys must not break the resource search index;
- `plankton password add --env NAME` must read the CLI process environment;
- access submission must become visible before ACP or API evaluation finishes;
- an interrupted evaluation must never leave the tray permanently busy;
- automatic LLM approval must appear as an ordinary request whose current
  status is “自动审批中” / “Automatic approval in progress”;
- the Plankton organism logo must stay static and must not be rotated as a
  loading indicator.

After implementation, rebuild, sign, install, and exercise the real macOS app
and CLI without exposing a password value in model-visible output.

## Resource Identity

`resource_fields.id` is a global database primary key. The current index writer
incorrectly reduces a resource URI to its trailing field segment, so
`first/password` and `second/password` collide.

The index writer will use the complete provider-neutral resource URI as the
field primary key and as `resource_search_documents.field_id`. Search data is a
rebuildable metadata index, so the first successful refresh replaces legacy
short IDs transactionally. Historical migration files remain unchanged.

## Environment Password Drafts

Environment variables belong to the process that launches the CLI. The CLI
therefore resolves only the names explicitly passed through repeated `--env`
flags and sends those selected name/value pairs to the authenticated loopback
daemon.

The transport uses a dedicated typed payload. Its `Debug` implementation
redacts values, daemon and CLI output DTOs contain only selected keys and draft
metadata, and neither side logs request bodies. Values may exist in trusted
local process memory and in the draft confirmation ledger, consistent with the
project's trusted-computer boundary. They must not be printed to stdout/stderr,
embedded in handoff URLs, audits, diagnostics, or model-visible results.

File descriptors remain path based because the CLI and daemon share the local
filesystem. Environment descriptors remain metadata-only when persisted or
shown to the human.

## Durable Automatic Approval

Access submission becomes a two-phase workflow:

1. Validate the request and backend availability.
2. Persist the access request immediately with `approval_status = pending`.
3. For manual mode, return the pending request.
4. For assisted or automatic mode, persist an evaluation operation associated
   with the request, return the request ID immediately, and run provider
   evaluation in a daemon-owned background task.
5. The CLI opens the desktop handoff and polls status using independent,
   bounded HTTP requests.
6. The worker writes the LLM suggestion and automatic decision, then finalizes
   the operation in all success and error paths.

Evaluation state is explicit:

- `not_required`
- `queued`
- `running`
- `completed`
- `failed`
- `interrupted`
- `superseded`

Queued and running automatic requests display “自动审批中”. Assisted requests
display “正在生成 AI 建议”. Provider failure leaves the request pending for a
human and displays the error; it is never silently converted into approval.

The human may approve or deny while evaluation is running. A human terminal
decision marks the evaluation superseded, and a late LLM result cannot replace
that decision.

## Recovery and Activity

The daemon worker updates an operation heartbeat while evaluating. A single
finalization boundary records completed, failed, interrupted, or superseded
state and is idempotent.

At daemon startup, stale running evaluations are marked interrupted and their
requests remain pending for human review. The tray activity query considers
only fresh queued/running evaluations. Stale records can therefore never keep
the application in a permanent loading state.

## Tray and Request UI

The idle tray icon remains the static Plankton logo.

During an active LLM evaluation, the icon switches to a dedicated standard
eight-frame circular spinner. The organism pixels are not rotated. macOS uses
a monochrome template spinner; Windows uses theme-appropriate light and dark
frames. Reduced-motion mode uses a static partial progress ring.

When evaluation completes but human action is still required, the tray returns
to the static Plankton logo with the attention badge.

The Requests page shows evaluation status independently from the final approval
decision. A new automatic request appears immediately, with “自动审批中” as its
primary status and provider progress in its detail pane.

## Error Handling

- Initial request persistence failures return a typed error and create no
  background job.
- Provider failures are persisted as diagnostics and leave the request
  available for human review.
- Worker panic/cancellation is finalized as interrupted.
- Status polling transport failures are explicit and retry only within a
  bounded policy.
- No failure is represented by an empty collection or discarded result.

## Verification

Automated tests must prove:

- two resources with the same trailing field key index and search correctly;
- selected CLI environment values reach a daemon with a different environment
  while all visible output remains value-free;
- a slow provider does not delay the initial pending response;
- the request is visible as automatic approval in progress during evaluation;
- success, failure, interruption, restart recovery, and human override all
  finalize operations;
- stale operations do not animate the tray;
- every reasoning frame is a spinner frame rather than a rotated organism;
- reduced motion is static;
- existing manual approval and file-draft paths remain green.

Installed-runtime verification must cover version, daemon health, list/search,
environment draft creation, human confirmation, automatic approval status,
approved get through a non-echoing verifier, tray recovery, and zero remaining
running operations.
