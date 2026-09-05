# Plankton Runtime Correctness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Correct the installed Plankton search, environment draft, automatic
approval, recovery, tray animation, and request-status behavior, then replace
and verify the local application.

**Architecture:** Use complete resource URIs as global field IDs; resolve
explicit environment values in the CLI and send them through a redacted typed
loopback payload; persist requests before daemon-owned background LLM
evaluation; drive UI and tray activity from explicit, recoverable evaluation
state.

**Tech Stack:** Rust, Tokio, Axum, Reqwest, SQLx/SQLite, React, TypeScript,
Tauri, Vitest.

## Global Constraints

- Work in the current checkout because the completed feature set is
  uncommitted and absent from `HEAD`.
- Preserve unrelated user and parallel-agent changes.
- Write each behavior test first and observe the expected failure.
- Never print a password value during tests or installed-runtime verification.
- Do not modify historical SQLx migration files.
- The Plankton organism logo must never rotate.

---

### Task 1: Repair Global Search Field Identity

**Files:**

- Modify: `crates/plankton-store/src/resources.rs`

**Interfaces:**

- Consumes: `ResourceDocument.resource_id`
- Produces: globally unique `resource_fields.id` and
  `resource_search_documents.field_id`

- [ ] Add a store test with `first/password` and `second/password`.
- [ ] Run the focused store test and observe the unique-primary-key failure.
- [ ] Bind the complete `document.resource_id` as both field IDs and remove the
      trailing-segment helper.
- [ ] Run store resource tests and daemon search contract tests.

### Task 2: Transfer Explicit Environment Values Across the Process Boundary

**Files:**

- Modify: `crates/plankton-protocol/src/passwords.rs`
- Modify: `crates/plankton-core/src/passwords/source.rs`
- Modify: `crates/plankton-client/src/lib.rs`
- Modify: `crates/plankton-daemon/src/routes/passwords.rs`
- Modify: `crates/plankton-cli/src/main.rs`
- Modify: `crates/plankton-daemon/tests/server_contract.rs`

**Interfaces:**

- Produces: a redacted typed password-draft input containing either a file
  descriptor or explicit selected environment entries
- Preserves: `PasswordDraftCreated` value-free receipt

- [ ] Add protocol and core tests proving `Debug` and metadata views redact
      values.
- [ ] Add a daemon contract test where the daemon environment lacks a sentinel
      but the client supplies it.
- [ ] Add CLI tests proving missing names fail before transport and all output
      formats omit values.
- [ ] Run the focused tests and observe the cross-process failure.
- [ ] Resolve selected values in the CLI and construct the typed input.
- [ ] Parse supplied entries in the daemon without consulting daemon
      environment.
- [ ] Run protocol, core, client, daemon, and CLI tests.

### Task 3: Persist Requests Before Automatic Evaluation

**Files:**

- Modify: `crates/plankton-core/src/domain.rs`
- Modify: `crates/plankton-store/src/sqlite.rs`
- Modify: `crates/plankton-store/src/read.rs`
- Modify: `crates/plankton-store/src/diagnostics.rs`
- Create: `crates/plankton-store/migrations/0008_async_evaluation.sql`
- Create: `crates/plankton-daemon/src/evaluation.rs`
- Modify: `crates/plankton-daemon/src/routes/resources.rs`
- Modify: `crates/plankton-daemon/src/state.rs`
- Modify: `crates/plankton-daemon/src/server.rs`
- Modify: `crates/plankton-daemon/src/lib.rs`

**Interfaces:**

- Produces: immediate pending `ResourceAccessResponse`
- Produces: explicit evaluation state on stored `AccessRequest`
- Preserves: approved values resolve only from the status route

- [ ] Add a daemon integration test with a blocked provider and assert the
      initial response arrives before provider release.
- [ ] Add store tests for queued/running/completed/failed/interrupted/
      superseded transitions and human-decision precedence.
- [ ] Run focused tests and observe that the current handler blocks.
- [ ] Add schema and typed evaluation state with backward-compatible defaults.
- [ ] Split request creation from provider evaluation.
- [ ] Start a daemon-owned worker only after durable request/operation commit.
- [ ] Heartbeat active work and finalize every terminal path idempotently.
- [ ] Recover stale work at startup as interrupted and retain human review.
- [ ] Run store and daemon tests.

### Task 4: Make CLI Polling Independent of Evaluation Duration

**Files:**

- Modify: `crates/plankton-cli/src/main.rs`
- Modify: `crates/plankton-client/src/lib.rs`

**Interfaces:**

- Consumes: immediate pending request ID
- Produces: bounded status polling until approval, denial, or explicit failure

- [ ] Add a CLI/client test where evaluation exceeds one HTTP timeout window.
- [ ] Observe the current initial request timeout.
- [ ] Keep the per-request timeout bounded and add bounded retry/backoff only
      to status polling.
- [ ] Verify CLI exit/cancellation does not cancel daemon evaluation.

### Task 5: Display Automatic Approval as Request State

**Files:**

- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/components/desktop/OperationsPages.tsx`
- Modify: `apps/desktop/src/components/desktop/OperationsPages.test.tsx`
- Modify: `apps/desktop/src/hooks/useDesktopApp.ts`
- Modify: `apps/desktop/src/i18n.ts`
- Modify: `apps/desktop/src/i18n.test.ts`

**Interfaces:**

- Consumes: request policy mode and evaluation state
- Produces: “自动审批中”, AI-advice progress, failure, and human-review labels

- [ ] Add component tests for automatic queued/running, assisted running,
      failure, completion-to-human, and human override.
- [ ] Run Vitest and observe missing status copy.
- [ ] Add typed state and localized labels.
- [ ] Refresh faster while an evaluation is active, then return to the normal
      interval.
- [ ] Run focused frontend tests, typecheck, and formatting.

### Task 6: Replace Rotating Logo with Dedicated Loading Frames

**Files:**

- Modify: `apps/desktop/src-tauri/src/tray.rs`
- Modify: `apps/desktop/src-tauri/src/main.rs`
- Modify: `apps/desktop/scripts/generate-icons.mjs`
- Modify: `apps/desktop/src-tauri/tests/icon_assets.rs`
- Modify generated tray assets under:
  `apps/desktop/src-tauri/assets/tray/generated/`

**Interfaces:**

- Consumes: fresh active evaluation state
- Produces: static brand icon for idle and dedicated spinner frames for
  reasoning

- [ ] Add Rust/icon tests proving brand pixels do not rotate and reasoning uses
      distinct spinner frames.
- [ ] Add tests proving stale/interrupted evaluations do not select reasoning.
- [ ] Observe existing quarter-rotation tests fail the desired contract.
- [ ] Generate monochrome macOS and themed Windows eight-frame spinner assets.
- [ ] Switch reasoning rendering to spinner assets; reduced motion uses a
      static ring.
- [ ] Run asset determinism, tray, and packaging tests.

### Task 7: Integrate, Build, Install, and Exercise the Real Runtime

**Files:**

- Modify only build-generated bundle contents outside the repository during
  installation.

**Interfaces:**

- Produces: `/Applications/Plankton.app`
- Produces: `/opt/homebrew/bin/plankton`

- [ ] Run Rust formatting, Clippy, workspace tests, frontend formatting,
      typecheck, tests, and production build.
- [ ] Build the release CLI and macOS app bundle.
- [ ] Verify bundled KeePassXC assets and strict-sign the final app.
- [ ] Stop the old process, replace the application and CLI atomically, and
      launch the new background app.
- [ ] Verify daemon health and zero stale running operations.
- [ ] Run list/search against repeated field keys.
- [ ] Create environment and file drafts using dummy values and verify no
      output contains those values.
- [ ] Observe the request UI show “自动审批中” during a deliberately slow
      evaluation.
- [ ] Complete an approved get through a non-echoing equality verifier.
- [ ] Verify the tray returns to idle and no running operation remains.
