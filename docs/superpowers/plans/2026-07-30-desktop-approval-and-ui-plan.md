# Plankton Desktop Approval and UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver silent automatic approval, focus-aware compact human review,
the maintained Codex ACP runtime, and a complete coherent desktop console,
then install and verify the real macOS application.

**Architecture:** Derive human-review presentation from domain state, expose it
through protocol read models, and let the persistent Tauri process choose a
compact or full React surface. Migrate Codex to the maintained ACP adapter and
unify the desktop under scoped design tokens, accessible page primitives, and
inline settings.

**Tech Stack:** Rust, Tokio, Axum, SQLx/SQLite, Tauri 2, React 19, TypeScript,
Vitest, Lucide React.

## Global Constraints

- Work in the current `dev` checkout because the requested prior feature set is
  uncommitted and absent from `HEAD`.
- Preserve unrelated user changes and do not reset or replace the dirty
  worktree.
- Write each behavior test first and observe the relevant failure.
- Automatic queued/running evaluation must never show or focus a window.
- Only a genuinely human-reviewable pending request may present approval UI.
- One compact window queues multiple requests; it never exposes secret values.
- The main window receives the full request only when visible, focused, and not
  minimized.
- Codex Latest is `npx -y @agentclientprotocol/codex-acp@latest`.
- Legacy pinned Zed adapter versions must not be mapped to new-package versions.
- Preserve the black, paper, and vermilion design and the static organism logo.
- Use named Lucide imports and retain full keyboard access.
- No caught error may be converted into an empty successful state.

---

### Task 1: Derive and Transport Human Review State

**Files:**

- Modify: `crates/plankton-core/src/domain.rs`
- Modify: `crates/plankton-protocol/src/resources.rs`
- Modify: `crates/plankton-store/src/read.rs`
- Modify: `crates/plankton-daemon/src/routes/resources.rs`
- Modify: `crates/plankton-daemon/tests/server_contract.rs`
- Modify: `crates/plankton-client/src/lib.rs`
- Modify: `crates/plankton-cli/src/main.rs`
- Modify: `crates/plankton-cli/src/desktop_handoff.rs`

**Interfaces:**

- Produces: `AccessRequest::human_review_required() -> bool`
- Produces: `ResourceAccessResponse.human_review_required: bool`
- Produces: one desktop handoff on the first `false -> true` status transition

- [ ] **Step 1: Add the failing domain matrix test.**

  Add table-driven cases for manual, assisted, automatic, every active/error
  evaluation state, and terminal approval. Assert the matrix from the design.

- [ ] **Step 2: Add failing daemon and CLI contract tests.**

  The daemon response must expose the derived field. A queued/running automatic
  response must never call the handoff abstraction; a later failed/pending
  status must call it exactly once.

- [ ] **Step 3: Run focused tests and record the expected failures.**

  Run:

  ```bash
  cargo test -p plankton-core human_review
  cargo test -p plankton-daemon --test server_contract human_review
  cargo test -p plankton handoff
  ```

- [ ] **Step 4: Implement one derivation and transport it.**

  Keep the decision pure:

  ```rust
  impl AccessRequest {
      pub fn human_review_required(&self) -> bool {
          self.approval_status.is_pending()
              && match self.policy_mode {
                  PolicyMode::ManualOnly => true,
                  PolicyMode::Assisted | PolicyMode::LlmAutomatic => {
                      !self.evaluation_status.is_active()
                  }
              }
      }
  }
  ```

  Adapt the exact enum names already present in the domain. Do not persist a
  duplicate boolean.

- [ ] **Step 5: Make CLI polling presentation-aware.**

  Track the previous derived value locally and dispatch only on the first
  transition. Continue polling silently while it is false.

- [ ] **Step 6: Run all affected Rust tests.**

  Run:

  ```bash
  cargo test -p plankton-core
  cargo test -p plankton-protocol
  cargo test -p plankton-client
  cargo test -p plankton-daemon
  cargo test -p plankton
  ```

### Task 2: Add Focus-Aware Compact Approval

**Files:**

- Create: `apps/desktop/src-tauri/src/approval_window.rs`
- Modify: `apps/desktop/src-tauri/src/main.rs`
- Modify: `apps/desktop/src-tauri/src/background.rs`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Create: `apps/desktop/src/components/CompactApproval.tsx`
- Create: `apps/desktop/src/components/CompactApproval.test.tsx`
- Modify: `apps/desktop/src/main.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/components/DesktopWorkspace.tsx`
- Modify: `apps/desktop/src/handoff.ts`
- Modify: `apps/desktop/src/handoff.test.ts`

**Interfaces:**

- Produces:
  `ApprovalSurface = FullMain | Compact | None`
- Produces: one reusable Tauri window labeled `approval`
- Consumes: daemon `human_review_required`

- [ ] **Step 1: Add failing Rust selection tests.**

  Cover visible/focused/minimized permutations and assert:

  ```rust
  select_surface(true, true, false, true) == ApprovalSurface::FullMain
  select_surface(true, false, false, true) == ApprovalSurface::Compact
  select_surface(false, false, false, true) == ApprovalSurface::Compact
  select_surface(true, true, false, false) == ApprovalSurface::None
  ```

- [ ] **Step 2: Add failing compact component tests.**

  Verify value-free request context, queue count, note editing, approve, reject,
  open-full-details, Escape/close semantics, and advancement to the next
  request.

- [ ] **Step 3: Run the focused tests and record failures.**

  Run:

  ```bash
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml approval
  npm --prefix apps/desktop test -- CompactApproval handoff
  ```

- [ ] **Step 4: Implement the window state machine.**

  Keep surface choice pure in `approval_window.rs`; schedule window operations
  outside synchronous event callbacks. Use `WebviewWindowBuilder` for a
  non-resizable `500×620` window and reuse it by label.

- [ ] **Step 5: Start background-first and deduplicate request presentation.**

  Set the main window `visible` flag to `false`. Explicit launch/tray action
  shows it. Human-review observation is deduplicated by request ID across the
  daemon watcher, single-instance callback, and deep link.

- [ ] **Step 6: Route each window to its own React surface.**

  Inspect the current Tauri window label at bootstrap. Render
  `CompactApproval` for `approval`; render the normal workspace for `main`.
  “Open full details” emits navigation with the request ID and focuses main.

- [ ] **Step 7: Verify lifecycle and frontend behavior.**

  Run:

  ```bash
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
  npm --prefix apps/desktop test -- CompactApproval DesktopWorkspace handoff
  npm --prefix apps/desktop run typecheck
  ```

### Task 3: Migrate and Readiness-Test Codex ACP

**Files:**

- Modify: `crates/plankton-core/src/acp.rs`
- Modify: `crates/plankton-core/tests/acp_configuration.rs`
- Modify: `apps/desktop/src/acpSettings.ts`
- Modify: `apps/desktop/src/acpSettings.test.ts`
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/components/desktop/OperationsPages.tsx`
- Modify: `apps/desktop/src/components/desktop/OperationsPages.test.tsx`

**Interfaces:**

- Produces: new Codex preset package
  `@agentclientprotocol/codex-acp`
- Produces: basic and readiness ACP probe results
- Preserves: pinned/custom semantics for Claude Code and OpenCode

- [ ] **Step 1: Add failing command-resolution and migration tests.**

  Assert Latest and pinned Codex selectors use the new package. Assert legacy
  Zed Latest migrates and legacy Zed pinned remains custom/deprecated.

- [ ] **Step 2: Add a failing readiness-probe test.**

  Use the existing fake ACP fixture or a deterministic test process. Make
  initialize succeed and the first prompt return a version incompatibility;
  assert the readiness probe reports the explicit incompatibility instead of
  claiming the connection is healthy.

- [ ] **Step 3: Run focused tests and record failures.**

  Run:

  ```bash
  cargo test -p plankton-core acp_
  npm --prefix apps/desktop test -- acpSettings OperationsPages
  ```

- [ ] **Step 4: Replace the Codex preset and legacy parser.**

  Use:

  ```rust
  const CODEX_ACP_PACKAGE: &str = "@agentclientprotocol/codex-acp";
  const LEGACY_CODEX_ACP_PACKAGE: &str = "@zed-industries/codex-acp";
  ```

  Never reuse a legacy pinned selector for the new package.

- [ ] **Step 5: Separate basic and model-readiness diagnostics.**

  Retain initialize metadata, then create a session and send a minimal
  value-free prompt. Return configured selector, adapter name/version, runtime
  metadata when available, readiness status, and full typed error text.

- [ ] **Step 6: Show both probes in Agents and Diagnostics.**

  Do not hide incompatibility or transport errors. Label which check failed and
  provide the actual configured command.

- [ ] **Step 7: Run ACP and frontend suites.**

  Run:

  ```bash
  cargo test -p plankton-core
  npm --prefix apps/desktop run test
  npm --prefix apps/desktop run typecheck
  ```

### Task 4: Unify the Workspace Foundation and Inline Settings

**Files:**

- Modify: `apps/desktop/package.json`
- Modify: `apps/desktop/package-lock.json`
- Create: `apps/desktop/src/components/desktop/icons.tsx`
- Create: `apps/desktop/src/components/desktop/PagePrimitives.tsx`
- Create: `apps/desktop/src/components/desktop/PagePrimitives.test.tsx`
- Modify: `apps/desktop/src/components/DesktopWorkspace.tsx`
- Modify: `apps/desktop/src/components/DesktopWorkspace.test.tsx`
- Modify: `apps/desktop/src/components/desktop/workspace.css`
- Modify: `apps/desktop/src/styles.css`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/App.test.tsx`
- Modify: `apps/desktop/src/components/desktop/OperationsPages.tsx`

**Interfaces:**

- Produces: scoped workspace tokens and page/dialog primitives
- Produces: inline Settings page using the existing settings state/actions
- Removes: Unicode navigation icons and the Settings modal

- [ ] **Step 1: Add Lucide and write failing navigation tests.**

  Install `lucide-react` through npm. Assert every navigation icon is SVG,
  decorative icon markup is hidden, and icon-only actions have names.

- [ ] **Step 2: Add failing layout and inline-settings tests.**

  Assert the workspace viewport scrolls, page empty/error states render next
  actions, settings categories are on-page, the Save bar is present, and no
  “Open configuration” button or settings modal remains.

- [ ] **Step 3: Run focused frontend tests and record failures.**

  Run:

  ```bash
  npm --prefix apps/desktop test -- DesktopWorkspace App PagePrimitives
  ```

- [ ] **Step 4: Introduce the scoped token system and primitives.**

  Define the design colors, typography, spacing, controls, focus ring, page
  header, empty/error state, split pane, pagination, dialog, and drawer under
  `.desktop-workspace`. Remove or rename colliding global selectors.

- [ ] **Step 5: Replace navigation and action glyphs.**

  Use named imports for Inbox, KeyRound, Cable, Bot, ScrollText, Activity,
  Settings, Search, ChevronLeft, ChevronRight, Eye, EyeOff, Copy, RefreshCw,
  X, Check, and Trash2. Keep the custom brand mark.

- [ ] **Step 6: Move settings into the page.**

  Reuse the current settings draft/save controller from `App` through explicit
  props or a focused hook. Render General, Approval, Providers, ACP, Passwords,
  and Sync categories with a sticky Save bar. Delete the obsolete modal path.

- [ ] **Step 7: Run frontend quality gates.**

  Run:

  ```bash
  npm --prefix apps/desktop run format
  npm --prefix apps/desktop run typecheck
  npm --prefix apps/desktop run test
  npm --prefix apps/desktop run build
  ```

### Task 5: Finish Every Workspace Page

**Files:**

- Modify: `apps/desktop/src/components/desktop/OperationsPages.tsx`
- Modify: `apps/desktop/src/components/desktop/OperationsPages.test.tsx`
- Modify: `apps/desktop/src/components/desktop/PasswordVaultPage.tsx`
- Modify: `apps/desktop/src/components/desktop/PasswordVaultPage.test.tsx`
- Modify: `apps/desktop/src/components/desktop/PasswordAddDialog.tsx`
- Modify: `apps/desktop/src/components/desktop/PasswordAddDialog.test.tsx`
- Modify: `apps/desktop/src/components/PasswordManagementView.tsx`
- Modify: `apps/desktop/src/components/desktop/workspace.css`
- Modify: `apps/desktop/src/i18n.ts`
- Modify: `apps/desktop/src/i18n.test.ts`

**Interfaces:**

- Consumes: Task 4 page primitives and icon wrappers
- Produces: complete Requests, Passwords, Connections, Agents, Audit,
  Diagnostics, and responsive interaction states

- [ ] **Step 1: Add failing request and pagination tests.**

  Cover no request, no match, selected request, long context, active
  evaluation, human review, and failure. Assert pagination is absent for zero
  or one page, present for two pages, and anchored after the list viewport.

- [ ] **Step 2: Add failing password interaction tests.**

  Cover search by title, notes, field key, and tags; fixed tag control height;
  reveal/hide/copy; bottom actions; conditional pagination; narrow filter
  drawer; long fields; empty/error states.

- [ ] **Step 3: Add failing operations page tests.**

  Cover connection groups and add drawer, centered Agents runtime status,
  Audit filters/detail, Diagnostics status/error list, and bilingual long copy.

- [ ] **Step 4: Run focused tests and record failures.**

  Run:

  ```bash
  npm --prefix apps/desktop test -- OperationsPages PasswordVaultPage PasswordAddDialog
  ```

- [ ] **Step 5: Implement Requests and Passwords.**

  Use full-height split panes, independent scroll regions, guided empty states,
  conditional pagination, separated field actions, sticky detail actions, and
  an accessible filter drawer at narrow widths.

- [ ] **Step 6: Implement Connections, Agents, Audit, and Diagnostics.**

  Remove fixed empty grid columns. Preserve complete errors, add the designed
  filters/status/detail affordances, and align actions consistently.

- [ ] **Step 7: Simplify legacy password management.**

  Namespace remaining legacy styles and split the oversized surface into
  Catalog, Add or Import, and Sources sections. Keep advanced templates
  collapsed and preserve confirmation before persistence.

- [ ] **Step 8: Run the entire frontend suite and build.**

  Run:

  ```bash
  npm --prefix apps/desktop run format
  npm --prefix apps/desktop run typecheck
  npm --prefix apps/desktop run test
  npm --prefix apps/desktop run build
  ```

### Task 6: Verify, Build, Install, and Exercise the Real App

**Files:**

- Modify only generated application bundle contents outside the repository
  during installation.

**Interfaces:**

- Produces: updated `/Applications/Plankton.app`
- Produces: updated `/opt/homebrew/bin/plankton`
- Produces: requirement-by-requirement verification evidence

- [ ] **Step 1: Run repository-wide gates.**

  Run:

  ```bash
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo test --workspace
  npm --prefix apps/desktop run format
  npm --prefix apps/desktop run typecheck
  npm --prefix apps/desktop run test
  npm --prefix apps/desktop run build
  ```

- [ ] **Step 2: Build release artifacts.**

  Build the release CLI and Tauri macOS bundle, including the bundled
  KeePassXC engine and icon asset tests.

- [ ] **Step 3: Replace installed artifacts safely.**

  Stop only verified Plankton processes, retain a recoverable backup of the
  installed app and CLI, install the new artifacts, sign the bundle, and
  launch the background app.

- [ ] **Step 4: Verify silent automatic approval.**

  Record the main and compact window state before and during a real automatic
  request. Assert neither window becomes visible or focused and the CLI reaches
  a terminal decision.

- [ ] **Step 5: Verify compact and full human review.**

  With main hidden or unfocused, create a manual request and assert only the
  compact window appears. With main focused, create another request and assert
  the full Requests detail appears without a compact window.

- [ ] **Step 6: Verify real Codex readiness.**

  Run the new adapter basic and readiness probes and one value-free suggestion.
  Capture adapter/runtime versions and ensure no old Zed package is launched.

- [ ] **Step 7: Walk all pages at four viewport sizes.**

  Capture Requests, Passwords, Connections, Agents, Audit, Diagnostics, and
  Settings at `1440×900`, `1024×768`, `820×740`, and `620×800`. Check empty,
  populated, error, long-content, keyboard, dialog, pagination, and scrolling
  states.

- [ ] **Step 8: Complete the requirement audit.**

  Map every design requirement to fresh source, automated-test, installed
  runtime, or screenshot evidence. Leave the goal active for every missing or
  indirect item.
