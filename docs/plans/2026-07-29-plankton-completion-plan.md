# Plankton Completion Implementation Plan

> Execute this plan continuously. Use test-driven development for behavior
> changes and verify each completed task before moving on.

**Goal:** Deliver the final daemon-backed, provider-neutral, AI-first
lightweight password manager defined in
`2026-07-29-plankton-completion-design.md`.

**Architecture:** A new `planktond` owns configuration, resource backends,
search, approvals, audit, ACP processes, and diagnostics. CLI and desktop are
clients of a versioned local protocol. A bundled KeePassXC/KDBX engine,
optional 1Password and Bitwarden adapters, and optional encrypted-vault sync
share one AI-facing resource model.

**Primary technologies:** Rust/Tokio/Axum/Reqwest/SQLx, React/TypeScript/Tauri,
Vitest, GitHub Actions.

## Task 1: Establish clean, pinned development gates

**Files:**

- Create: `rust-toolchain.toml`
- Create: `.nvmrc`
- Modify: `Cargo.toml`
- Modify: `Makefile`
- Modify: `apps/desktop/package.json`
- Modify: `apps/desktop/vite.config.ts`
- Delete: `apps/desktop/bun.lock`
- Modify: `.github/workflows/ci.yml`

**Steps:**

1. Add a failing CI/config assertion for supported Rust and Node versions and
   the single npm lockfile.
2. Pin stable Rust components `rustfmt` and `clippy`; pin the supported Node LTS.
3. Upgrade the frontend test stack as needed so Node does not inject an
   incompatible experimental WebStorage global.
4. Add `make lint` and make `make check` cover formatter, Clippy with
   `-D warnings`, Rust tests, frontend format/type/test/build.
5. Make CI run for `dev`, `main`, and pull requests and include every workspace
   member.
6. Run the new gates and fix only baseline failures required to make the gate
   authoritative.

## Task 2: Define shared protocol and typed errors

**Files:**

- Create: `crates/plankton-protocol/Cargo.toml`
- Create: `crates/plankton-protocol/src/lib.rs`
- Create: `crates/plankton-protocol/src/error.rs`
- Create: `crates/plankton-protocol/src/resources.rs`
- Create: `crates/plankton-protocol/src/daemon.rs`
- Modify: `Cargo.toml`

**Steps:**

1. Write serialization tests for protocol versioning, request correlation IDs,
   AI-safe versus human errors, pagination cursors, partial-result warnings,
   ACP profiles, and daemon state.
2. Implement DTOs with `deny_unknown_fields` on requests and stable error codes.
3. Implement explicit AI-safe projections that cannot serialize backend source
   fields.
4. Add compatibility tests proving unknown operations and versions fail closed.

## Task 3: Add daemon persistence models

**Files:**

- Create: `crates/plankton-store/migrations/0006_daemon_resources.sql`
- Create: `crates/plankton-store/migrations/0007_diagnostics.sql`
- Create: `crates/plankton-store/src/resources.rs`
- Create: `crates/plankton-store/src/diagnostics.rs`
- Modify: `crates/plankton-store/src/lib.rs`
- Modify: `crates/plankton-store/src/sqlite.rs`

**Steps:**

1. Write migration tests from an empty database and a version-5 fixture.
2. Add KDBX Vault manifests, resource aliases, backend locators, password
   drafts, confirmation tokens, search documents, sync state, diagnostics, and
   interrupted-request tables.
3. Add server-side request/audit/resource query objects with filtering,
   deterministic sort, and keyset pagination.
4. Test transaction rollback, stale cursor behavior, error acknowledgement, and
   interrupted request recovery.

## Task 4: Implement the password-manager domain and search core

**Files:**

- Create: `crates/plankton-core/src/resources/mod.rs`
- Create: `crates/plankton-core/src/resources/backend.rs`
- Create: `crates/plankton-core/src/resources/local.rs`
- Create: `crates/plankton-core/src/resources/search.rs`
- Create: `crates/plankton-core/src/passwords/model.rs`
- Create: `crates/plankton-core/src/passwords/migration.rs`
- Modify: `crates/plankton-core/src/lib.rs`
- Refactor: `crates/plankton-core/src/value_resolver.rs`
- Modify: `crates/plankton-core/Cargo.toml`

**Steps:**

1. Write model tests for Vault, group, Item, Section, Field, tags, ordering,
   aliases, local CRUD, history, archive, move, duplicate, and delete.
2. Write legacy migration tests for flat literals and grouped imported fields.
3. Write search tests for identifier, display name, aliases, notes, field
   key/label/section, tags, metadata, Unicode, CJK, typo tolerance, weighting,
   filters, pagination, and stable ordering.
4. Write merged-backend tests proving AI results omit backend identity and
   duplicates receive distinct stable IDs.
5. Introduce `CredentialBackend`, capability negotiation, bindings, and search
   documents.
6. Implement normalized fuzzy matching with a maintained dependency.
7. Replace the CLI-local substring search with the core search service.

## Task 5: Integrate the bundled KeePassXC/KDBX local engine

**Files:**

- Create: `crates/plankton-core/src/resources/keepassxc.rs`
- Create: `crates/plankton-core/src/resources/keepassxc_command.rs`
- Create: `crates/plankton-core/src/resources/unlock.rs`
- Create: `crates/plankton-core/tests/fixtures/fake-keepassxc-cli`
- Create: `engines/keepassxc/manifest.json`
- Create: `engines/keepassxc/README.md`
- Create: `scripts/fetch-keepassxc-engine.*`
- Modify: release packaging scripts and workflows

**Steps:**

1. Write fake-engine tests for version/hash checks, create, unlock, list, show,
   add, edit, move, remove, groups, tags, attributes, history, merge, stderr,
   malformed output, timeout, cancellation, and corruption.
2. Implement a typed argv-only sidecar adapter and serialize writes per Vault.
3. Map Vault/group/Item/Field/Tag/history to KDBX and version the Plankton
   Section-layout custom attribute.
4. Implement atomic copy-modify-validate-replace writes and backups.
5. Implement OS credential storage and optional passphrase/key-file unlock.
6. Pin engine versions and checksums per platform; include GPL licenses,
   corresponding source revision, source offer, and reproducible build recipe.
7. Add Windows, macOS, and Linux packaging and smoke tests.

## Task 5A: Implement optional 1Password and Bitwarden backends

**Files:**

- Create: `crates/plankton-core/src/resources/onepassword.rs`
- Create: `crates/plankton-core/src/resources/onepassword_command.rs`
- Create: `crates/plankton-core/src/resources/bitwarden.rs`
- Create: `crates/plankton-core/src/resources/bitwarden_command.rs`
- Create: `crates/plankton-core/tests/fixtures/fake-op`
- Create: `crates/plankton-core/tests/fixtures/fake-bw`
- Refactor: `crates/plankton-core/src/value_resolver.rs`
- Refactor: `apps/desktop/src-tauri/src/import_browse.rs`

**Steps:**

1. Write table-driven tests enumerating every supported native command,
   AI-readable subcommand, rejected AI write, file-writing flag, and unknown
   flag for both managers.
2. Write fake-CLI tests for signed-out state, malformed JSON, stderr, non-zero
   exit, timeout, cancellation, multi-vault sync, notes, tags, field keys,
   documents, and partial failure.
3. Implement direct argv execution and exact allowlists; never invoke a shell.
4. Implement disabled-by-default configuration, health, capability reporting,
   scoped incremental sync, completeness, stable IDs, and get for both.
5. Migrate existing references to grouped internal locators and preserve
   access.
6. Implement management writes only behind daemon-issued, single-use human
   confirmation tokens.
7. Ensure every AI projection and error is provider-neutral.

## Task 6: Implement `planktond`

**Files:**

- Create: `crates/plankton-daemon/Cargo.toml`
- Create: `crates/plankton-daemon/src/main.rs`
- Create: `crates/plankton-daemon/src/server.rs`
- Create: `crates/plankton-daemon/src/state.rs`
- Create: `crates/plankton-daemon/src/routes/*.rs`
- Create: `crates/plankton-daemon/src/launchd.rs`
- Create: `crates/plankton-client/Cargo.toml`
- Create: `crates/plankton-client/src/lib.rs`
- Modify: `Cargo.toml`

**Steps:**

1. Write protocol integration tests for authentication, version mismatch,
   correlation, startup, shutdown, restart, interrupted request recovery,
   cancellation, timeout, and error persistence.
2. Implement atomic state-file creation and loopback server startup.
3. Implement health, resources, requests, audit, diagnostics, settings,
   connections, and ACP routes.
4. Move approval and resolver orchestration into daemon application state.
5. Implement the typed client with retry limited to idempotent operations.
6. Implement macOS LaunchAgent install/uninstall/start/stop/status/logs and
   foreground mode.

## Task 7: Convert CLI into a daemon client

**Files:**

- Split: `crates/plankton-cli/src/main.rs`
- Create: `crates/plankton-cli/src/commands/*.rs`
- Create: `crates/plankton-cli/src/output.rs`
- Create: `crates/plankton-cli/src/error.rs`
- Refactor: `crates/plankton-cli/src/desktop_handoff.rs`
- Modify: `crates/plankton-cli/Cargo.toml`

**Steps:**

1. Write CLI tests for daemon absence, startup failure, human stderr, JSON
   errors, search filters, pagination, get approval, cancellation, daemon
   management, and exit codes.
2. Route list/search/get/request/audit/settings through `plankton-client`.
3. Add structured enhanced search flags while preserving compatible basic
   invocations.
4. Remove process-local SQLite polling and value resolution.
5. Remove dead status/report models and make desktop handoff wait for and report
   launcher failure.

## Task 7A: Implement password-add drafts and source parsers

**Files:**

- Create: `crates/plankton-core/src/passwords/draft.rs`
- Create: `crates/plankton-core/src/passwords/source.rs`
- Create: `crates/plankton-cli/src/commands/password.rs`
- Create: `crates/plankton-daemon/src/routes/passwords.rs`
- Modify: daemon resource migrations

**Steps:**

1. Write tests for repeated environment variables, `.env`, JSON, YAML, escaped
   dotted paths, JSON Pointer, omitted-key trees, scalar enforcement, missing
   files, permissions, malformed input, and duplicate keys.
2. Implement strict source descriptors and parsers without implicit bulk
   import.
3. Persist `PasswordDraft` snapshots, source context, correlation IDs, expiry,
   and status.
4. Implement desktop handoff and CLI wait/cancel behavior.
5. Bind single-use confirmation tokens to the normalized final draft,
   destination, operation, and value hash.
6. Test replay, mutation after confirmation, daemon restart, backend failure,
   partial-write prevention, and audit records.

## Task 7B: Implement optional encrypted-vault synchronization

**Files:**

- Create: `crates/plankton-core/src/sync/mod.rs`
- Create: `crates/plankton-core/src/sync/local_folder.rs`
- Create: `crates/plankton-core/src/sync/git.rs`
- Create: `crates/plankton-core/src/sync/webdav.rs`
- Create: `crates/plankton-core/src/sync/http_blob.rs`
- Create: `crates/plankton-daemon/src/routes/sync.rs`

**Steps:**

1. Write adapter contract tests proving only encrypted KDBX blobs and bounded
   manifests leave the daemon.
2. Implement local-folder synchronization with file watching and atomic copies.
3. Implement Git snapshot synchronization over SSH/HTTPS without textual KDBX
   merges.
4. Implement WebDAV and custom HTTP blob synchronization with ETag/generation
   preconditions.
5. Implement base/local/remote preservation and KeePassXC merge on divergence.
6. Test offline retry, concurrent writers, invalid remote blobs, auth failure,
   merge failure, tombstones, history, and recovery.

## Task 8: Centralize ACP latest and pinned profiles

**Files:**

- Modify: `Cargo.toml`
- Refactor: `crates/plankton-core/src/acp.rs`
- Refactor: `crates/plankton-core/src/config.rs`
- Delete or reduce: `apps/desktop/src/acpSettings.ts`
- Modify: `apps/desktop/src/acpSettings.test.ts`
- Modify: `crates/plankton-daemon/src/routes/acp.rs`

**Steps:**

1. Write config tests for Codex, Claude Code, and OpenCode latest presets,
   exact pins, invalid versions, custom commands, and legacy migration.
2. Update the ACP SDK and adapt compilation/API behavior.
3. Generate launch argv from one Rust source of truth.
4. Capture stderr, await I/O tasks, monitor child exit, and persist diagnostics.
5. Implement connection test and resolved runtime-version reporting.
6. Remove hardcoded frontend ACP defaults.

## Task 9: Rebuild desktop application structure

**Files:**

- Delete: `apps/desktop/src/main.ts`
- Split: `apps/desktop/src/App.tsx`
- Split: `apps/desktop/src/hooks/useDesktopApp.ts`
- Split: `apps/desktop/src/components/PasswordManagementView.tsx`
- Create: `apps/desktop/src/app/router.tsx`
- Create: `apps/desktop/src/layout/AppShell.tsx`
- Create: `apps/desktop/src/pages/*.tsx`
- Create: `apps/desktop/src/features/*`
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/i18n.ts`
- Modify: `apps/desktop/src/styles.css`
- Refactor: `apps/desktop/src-tauri/src/main.rs`
- Create: `apps/desktop/src-tauri/assets/tray/*`
- Create: `apps/desktop/src-tauri/src/tray.rs`
- Create: `apps/desktop/src-tauri/src/background.rs`

**Steps:**

1. Write component tests for persistent navigation and every empty, loading,
   partial, success, and error state.
2. Implement typed daemon invocations in Tauri; remove duplicated business
   orchestration.
3. Implement a three-pane Passwords workspace with Vault/group/tag navigation,
   pageable Items, Item/Section/Field editor, history, create, edit, duplicate,
   move, archive, and delete.
4. Implement the password-add popup with parsed source tree, field selection,
   editable metadata, new/existing Item choice, Section mapping, destination,
   duplicate policy, reveal control, and final confirmation.
5. Implement Requests with tabs, filters, search, sort, pagination, keyboard
   navigation, and a detail pane.
6. Integrate fuzzy query, tag/key/note filters, match details, pagination,
   aliases, and human-only source data.
7. Implement Connections with 1Password and Bitwarden off by default,
   LocalFolder/Git/WebDAV/HTTP sync, setup, completeness, conflicts, refresh,
   health, capabilities, disable, and failure recovery.
8. Implement Agents & Models, pageable Audit, persistent Diagnostics, and full
   Settings pages.
9. Make the desktop app a persistent background owner on macOS and Windows:
   close hides, CLI handoff reuses the live process, and tray actions open
   Requests/Passwords/Diagnostics or explicitly quit.
10. Generate and package the flat Plankton/keyhole icon as a macOS monochrome
    Template Image and Windows multi-size tray/taskbar icon. Drive
    idle/attention/reasoning/degraded/disconnected states from daemon events;
    animate reasoning for ACP/API inference and honor reduced motion.
11. Add lifecycle and icon-state tests for cold launch, hidden-window handoff,
    single instance, explicit quit, daemon loss, inference start/finish/failure,
    and macOS/Windows packaging.
12. Preserve the visual identity while fixing hierarchy, focus, narrow layouts,
   reduced motion, and Chinese localization.

## Task 10: Remove muted errors and legacy paths

**Files:**

- Modify all matches identified by:
  `rg -n '\\.ok\\(\\)|let _ =|if let Ok|Stdio::null|unwrap_or_default' crates apps`
- Refactor: `apps/desktop/src-tauri/src/import_browse.rs`
- Refactor: `apps/desktop/src-tauri/src/main.rs`
- Refactor: `crates/plankton-core/src/acp.rs`
- Refactor: `crates/plankton-cli/src/desktop_handoff.rs`

**Steps:**

1. Add regression tests for each previously muted operational failure.
2. Classify true NotFound/optional cases explicitly.
3. Propagate, persist, and display every other error or warning.
4. Run dead-code and duplicate-default searches and delete obsolete paths.
5. Split large modules until domain boundaries are clear and no replacement
   leaves an unused legacy implementation.

## Task 11: Complete CI, release, documentation, and runtime QA

**Files:**

- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release-cli.yml`
- Modify: `README.md`
- Modify: `docs/p1-runbook.md` without overwriting unrelated user edits
- Create: `docs/daemon.md`
- Create: `docs/resources-and-search.md`
- Create: `docs/onepassword.md`
- Create: `docs/diagnostics.md`
- Create: `docs/migration.md`

**Steps:**

1. Make release depend on a full verification job for the tagged commit.
2. Add macOS daemon and Tauri smoke coverage with a fake backend.
3. Document trusted-local security, daemon lifecycle, AI-safe resource API,
   search syntax, optional 1Password setup, ACP version modes, errors, backup,
   migration, and recovery.
4. Run `make fmt`, then `make check` from a clean tracked state.
5. Run focused daemon, backend, search, ACP, migration, and UI test commands
   with uncaptured output.
6. Build the CLI, daemon, frontend, and macOS desktop bundle.
7. Exercise daemon status, merged search, approval/get, diagnostics, ACP tests,
   and 1Password disabled state using isolated temporary configuration.

## Task 12: Requirement-by-requirement completion audit

**Steps:**

1. Map every explicit user requirement and every design acceptance item to a
   source file, test, command output, or runtime observation.
2. Search for obsolete defaults, old entry points, provider leakage in AI DTOs,
   write-capable backend operations, muted errors, dead code warnings, missing
   pagination, and untested routes.
3. Treat missing or indirect evidence as incomplete and fix it.
4. Re-run the complete gate after the last change.
5. Review the final diff without staging unrelated user changes.
6. Mark the active goal complete only when every row has authoritative passing
   evidence.
