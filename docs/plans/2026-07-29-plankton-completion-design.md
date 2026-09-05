# Plankton Completion Design

**Date:** 2026-07-29

## Goal

Complete Plankton as a daemon-backed approval and secret-access system instead
of extending the current process-local prototype. The completed system must:

- operate as an AI-first lightweight password manager whose human interaction
  model uses vaults, items, sections, fields, notes, and tags;
- present one provider-neutral Plankton resource model to AI clients;
- keep locally stored secrets as the always-available built-in backend;
- make external password managers disabled-by-default, user-enabled backends;
- ship first-class Local, 1Password, and Bitwarden backends behind a common
  CLI/API adapter registry;
- let AI submit password-add drafts from environment variables and `.env`,
  JSON, or YAML files while requiring a human popup for every actual write;
- support complete fuzzy search over identifiers, names, notes, field keys,
  tags, and metadata;
- run approval, resolution, ACP, audit, and diagnostics through one daemon;
- default ACP presets to `latest` while allowing exact version pins;
- surface every operational failure through a typed error channel;
- remove obsolete entry points, duplicate implementations, and dead models;
- provide a navigable desktop UI with pagination, filtering, diagnostics, and
  accessible interactions;
- prove the result with unit, integration, UI, build, lint, and migration tests.

There is no staged "first version" in this design. The acceptance criteria
describe the final state.

## Trust and Security Boundary

The user's computer, Plankton configuration, Plankton database, local catalog,
and persisted secret values are trusted. Persisting plaintext credentials,
provider tokens, provider metadata, resolved values, and diagnostic output is
therefore permitted and is not treated as a vulnerability.

The boundaries that remain enforced are:

1. AI clients may search, list, and get resources, and may submit a password-add
   draft. A draft is not permission to write.
2. Secret reads pass through Plankton policy, approval, and audit.
3. An optional backend cannot add direct write operations to the AI surface.
   Backend writes require a single-use human confirmation bound to the final
   draft and destination.
4. The command approved must be the command executed.
5. Unknown protocol operations, backend commands, and backend flags fail
   closed.
6. Remote transmission outside an explicitly configured model/provider request
   is not allowed.
7. AI-facing data must not reveal whether a resource is local, 1Password-backed,
   or supplied by a future backend.

Human diagnostics may show backend names, locators, commands, and raw stderr.
The human desktop UI may create, edit, move, archive, and delete password data.
Those human operations are distinct from the AI protocol.

## Runtime Architecture

Add a `planktond` workspace crate. It is the single owner of:

- configuration and schema migration;
- SQLite request and audit stores;
- resource catalog and search index;
- backend lifecycle and resource resolution;
- manual, assisted, and automatic approval orchestration;
- ACP subprocess lifecycle;
- error persistence and diagnostics;
- cancellation and timeout state.

The CLI and Tauri desktop become clients. They communicate with `planktond`
over loopback HTTP using a persisted local bearer token. The listener binds
only to `127.0.0.1` on a dynamically allocated persisted port. A state file
contains the endpoint, token, daemon PID, protocol version, and start time.
Atomic state-file replacement prevents clients from reading a partial record.

The token is not intended to defend against another local user in the trusted
environment. It prevents accidental cross-process and cross-profile use and
supports protocol correlation.

On macOS, `plankton daemon install` installs a LaunchAgent and
`plankton daemon start|stop|restart|status|logs` manages it. A foreground
`plankton daemon run` command supports development and other platforms.
Clients may start an installed daemon when it is absent, but a startup failure
is reported rather than hidden.

Every request has a correlation ID. The daemon writes state transitions before
responding to clients, so restart recovery can classify an interrupted request
instead of leaving it permanently pending.

## Lightweight Password Manager Model

Replace the flat `resource -> value or source locator` catalog with:

```text
Vault
└── Item
    ├── notes
    ├── tags
    └── Section
        └── Field
            ├── key
            ├── label
            ├── type
            └── value
```

A Vault is a Plankton organization and destination boundary. An Item is the
unit users browse and manage. Sections order related fields. Fields contain
typed scalar values and are the smallest retrievable AI resource. Tags belong
to Items and support slash-separated hierarchy.

The internal model includes:

- `Vault`: ID, name, description, backend binding, ordering, timestamps;
- `Item`: ID, vault ID, category, title, notes, tags, aliases, timestamps;
- `Section`: ID, item ID, title, ordering;
- `Field`: ID, section ID, key, label, type, value locator, ordering;
- `ResourceAlias`: old or human-friendly resource ID to stable Field ID;
- `BackendBinding`: backend ID and opaque provider locator.

The Local backend stores authoritative password data in standard KDBX4 vault
files through a bundled KeePassXC command-line sidecar. SQLite stores only
Plankton workflow state, derived search documents, aliases, backend bindings,
and diagnostics. Persisted plaintext remains permitted by the trusted-local
boundary, but the authoritative local password database is encrypted so copied
vault files remain protected and interoperable.

1Password maps account/vault/item/section/field into this structure. Bitwarden
maps a personal or organization vault to Vault, folder or collection hierarchy
to an optional collection path, and item/custom fields to Item and Field.
Backend-specific ownership and permissions remain internal.

Human UI behavior follows this model: browse vaults, inspect an item, create and
reorder sections and fields, apply tags, move or duplicate items, archive, and
delete with confirmation. Plankton does not add browser autofill, passkey
management, family sharing, breach monitoring, or other full-suite password
manager features.

## Bundled Open-Source Local Engine

Plankton packages a pinned, checksummed KeePassXC CLI sidecar for supported
macOS, Windows, and Linux targets. Users do not need a separate KeePassXC
installation. The daemon locates only its bundled engine by default, verifies
the manifest and SHA-256 before first use, and reports a typed fatal diagnostic
when the engine is absent or invalid.

The engine boundary is a subprocess protocol rather than a linked library:

- KeePassXC remains an independently licensed GPL-2/GPL-3 component;
- distributions include its license, notices, exact source revision, build
  recipe, and corresponding source offer;
- Plankton never copies or reimplements its cryptographic routines;
- stdout, stderr, exit status, timeout, cancellation, and engine version are
  captured and audited;
- no shell command construction is used.

Plankton maps its model to KDBX as follows:

```text
Plankton Vault       -> KDBX database
CollectionPath       -> KeePassXC group path
Item                 -> KeePassXC entry
username/password/url/notes -> standard entry fields
other Field          -> protected additional attribute
Tag                  -> entry tag
Section/order layout -> versioned Plankton custom-data attribute
history              -> KeePassXC entry history
```

New local vaults use KDBX4, the engine's recommended Argon2 configuration,
integrity protection, atomic safe-save behavior, backups, entry UUIDs, history,
and deletion tombstones. The daemon serializes writes per vault and operates on
temporary copies before atomically replacing the authoritative file.

The default unlock secret is randomly generated and persisted in the operating
system credential facility: macOS Keychain, Windows Credential Manager/DPAPI,
or Linux Secret Service. This provides automatic unlock under the trusted-local
model. Users may instead require a passphrase, key file, or both. A missing
credential facility is an explicit setup error; Plankton never silently creates
an unencrypted vault.

The pure Rust `keepass` crate is not the authoritative writer because its KDBX
write support is currently experimental. It may be used only for independent
read-only validation tests if doing so adds useful differential coverage.

## Provider-Neutral AI Resource Model

AI clients use only these domain operations:

- `resources.search`
- `resources.list`
- `resources.get`

Each Field has a stable Plankton resource ID. The AI DTO contains:

- stable Plankton resource ID;
- display name and optional aliases;
- provider-neutral description/notes;
- tags;
- field key/label when the result represents a field;
- metadata safe for the Plankton resource directory;
- match information and pagination cursor.

It never contains provider kind, vault, account, provider item ID, source
locator, executable, or backend-specific stderr.

Internally, a resource record also contains a backend ID and opaque locator.
`CredentialBackend` provides:

```text
capabilities()
sync(cursor)
search_documents()
get(locator)
health()
```

The interface advertises granular capabilities such as search, get, create
item, update item, move, archive, delete, history, and sync.
`KeePassLocalBackend` is always enabled. `OnePasswordBackend` and
`BitwardenBackend` are registered only when the user enables them. Other CLI or
API password managers implement the same adapter contract without changing the
public domain model.

Read capabilities are callable by the approval pipeline. Write capabilities
are callable only with a daemon-issued human confirmation token. Backends
cannot expose arbitrary native commands through this interface.

Backend failures are translated to provider-neutral AI errors. The full error
chain remains available to the human diagnostics API.

## Optional External Password Manager Backends

1Password and Bitwarden are off by default. When off, Plankton does not probe
their CLIs or APIs, show setup prompts outside the Connections page, or change
local resource behavior.

Enabling the backend is an explicit human action:

1. verify the CLI and signed-in account;
2. select accounts and vault scopes;
3. perform an initial metadata sync;
4. show completeness and any excluded or failed scope;
5. persist the enabled state and sync cursor.

The 1Password adapter invokes `op` directly without a shell. Its AI-readable
allowlist is:

- `account list`, `account get`;
- `vault list`, `vault get`;
- `item list`, `item get`;
- `document list`, `document get`;
- `read`;
- `whoami`.

AI clients do not invoke those commands. They invoke Plankton resource
operations, and the adapter selects a validated backend operation internally.

File-writing flags such as `--out-file`, `--force`, and `--file-mode` are
rejected. Share-link creation is rejected. Unknown commands and flags are
rejected.

AI-initiated writes are rejected. Human-confirmed management uses separately
validated create, edit, move, archive, and delete operations. These operations
require a single-use confirmation token bound to the normalized final item,
destination, operation, and value hash. Bitwarden CLI/API operations follow the
same separation.

The executable is launched with an argv array. The normalized operation and
argument hash are bound to the approval record before execution. stdout,
stderr, exit status, timeout, cancellation, and parse failures are recorded.

Existing imported 1Password references remain valid. When those records exist,
migration treats the backend as previously user-enabled, preserving existing
behavior. A fresh or purely local installation remains disabled.

Existing imported Bitwarden references receive the equivalent migration.

## Password Add Draft Workflow

Add a CLI command that accepts:

```text
plankton password add --env NAME
plankton password add --file PATH --key KEY
```

`--env` and `--key` may repeat. `.env`, `.json`, `.yaml`, and `.yml` are
supported. JSON and YAML keys accept escaped dotted paths and JSON Pointer.
Only scalar values can become Fields.

If a structured file is supplied without keys, the daemon parses it and the
popup presents a tree for explicit field selection. It never silently imports
every value. File-not-found, permission, parser, missing-key, non-scalar, and
duplicate-key errors are typed and visible.

The CLI sends source descriptors to the daemon. The daemon reads and snapshots
the values into a persisted `PasswordDraft`, records the requesting call chain,
and opens the desktop confirmation route. The CLI may wait, print a request ID,
or cancel its wait without discarding the draft.

The popup lets the human select values, edit Item metadata, choose a Vault and
enabled backend, create a new Item or append to an existing one, map Sections
and Fields, reveal or edit values, choose duplicate handling, and confirm the
final write.

There is no `--yes`, auto-approval, policy bypass, or headless write. Changing
the form invalidates an earlier confirmation. The daemon executes only the
exact confirmed write and records the result.

## Optional Encrypted Vault Sync

Synchronization transports only encrypted KDBX files and provider-neutral
manifests. Unlock keys and plaintext are never uploaded by the sync layer.

Built-in sync adapters are:

1. `LocalFolderSync` for iCloud Drive, Dropbox, OneDrive, Syncthing, NAS, and
   other filesystem synchronization;
2. `GitSync` for a user-selected repository, remote, and branch over SSH or
   HTTPS;
3. `WebDavSync`;
4. `HttpBlobSync` for a custom service implementing manifest and blob GET/PUT
   with generation or ETag preconditions.

The manifest contains only schema version, Vault ID, generation, encrypted
blob hash, byte length, and update time. Authentication values are stored as
ordinary Plankton credentials and resolved only inside the daemon.

Every write uses optimistic concurrency. When local and remote both changed,
the daemon retains base/local/remote snapshots and invokes KeePassXC database
merge. KeePassXC entry UUIDs and modified timestamps select the current
version, older versions enter entry history, new groups and entries are added,
and deletion tombstones prevent removed entries from reappearing. The merged
vault is validated and atomically installed before a new remote generation or
Git commit is published.

Git treats KDBX as an opaque binary artifact; it never attempts a textual
merge. Commit history provides encrypted snapshots. Credentials, remote
errors, conflicts, retry state, and last successful synchronization are visible
in the human UI and diagnostics. Automatic sync uses bounded exponential
backoff and never discards a conflicting copy.

## Search Design

The current ASCII-lowercase substring filter is replaced with a
provider-neutral search service and rebuildable local index.

Each `ResourceSearchDocument` includes:

- resource ID;
- display name and aliases;
- description and notes;
- tags;
- every field key, label, and section;
- metadata keys and values;
- internal backend locator, excluded from AI serialization;
- synchronization and usage timestamps.

1Password sync reads item metadata and field descriptions for enabled scopes.
Each readable field receives a stable Plankton resource ID. Item notes and tags
are inherited by field documents. Secret values are resolved only by `get`;
they are not required to build the search index.

Normalization includes Unicode NFKC, case folding, separator tokenization, and
stable whitespace handling. Matching uses a maintained fuzzy-matching library
instead of a custom edit-distance implementation.

Weights, from highest to lowest, are:

1. exact resource ID and exact display name;
2. display-name and alias fuzzy match;
3. tag and field key/label match;
4. description, notes, and metadata match.

All free-text query tokens must match, but tokens may match different indexed
fields. Structured filters support:

- repeated `tag_all`;
- repeated `tag_any`;
- field-key query;
- notes query;
- stable sort;
- limit and opaque cursor.

Results include score, provider-neutral `matched_on`, and bounded highlights.
Ordering is deterministic: relevance, exactness, recent successful access, and
resource ID. Cursors include the index generation so a changed index returns a
clear stale-cursor error instead of silently skipping results.

The UI shows sync completeness. During a partial backend failure, local and
successfully indexed resources remain searchable, the AI response reports a
provider-neutral partial-result warning, and human diagnostics contain the
failed scopes and raw errors.

## Approval and Audit

Search, list, and get use the same Plankton request context and policy engine.
Policy may automatically approve low-sensitivity directory reads, but it may
not bypass the request and audit path. Secret value retrieval is always bound
to a recorded decision.

Audit records capture:

- normalized Plankton operation;
- resource IDs and query/filter shape;
- call-chain and requesting agent;
- decision mode, actor, explanation, and model trace;
- exact backend-operation hash;
- start/end time, outcome, exit status, and correlation ID;
- warnings and errors.

Backend identity is available in the human audit DTO only.

## ACP Version Model

Replace duplicated program/argument defaults with one structured Rust model:

```text
agent_kind: codex | claude_code | opencode | custom
version_mode: latest | pinned
version: optional exact version
program: optional custom executable
args: optional custom arguments
```

Built-in launchers are:

- Codex: `npx -y @zed-industries/codex-acp@latest`
- Claude Code: `npx -y @zed-industries/claude-code-acp@latest`
- OpenCode: `npx -y opencode-ai@latest acp`

Pinned mode substitutes an exact validated version. Custom mode preserves
explicit program/args. The desktop imports the serialized preset information
from the daemon rather than maintaining a second TypeScript default table.

The historical shipped Codex `0.11.1` default migrates to latest. An explicit
user-selected semantic version remains pinned. The Rust ACP SDK is updated to
the current compatible release and adapted to its API.

ACP stderr is captured in a bounded ring buffer. I/O task termination, child
exit, initialization failure, cancellation, and protocol errors all reach
diagnostics. The UI provides Test Connection and displays both the configured
selector and resolved runtime version.

## Error Model

All layers use a stable error envelope:

```text
code
user_message
internal_message
context
severity
retryable
timestamp
correlation_id
source
```

CLI human output writes a concise message to stderr and exits non-zero. JSON
output returns the envelope. AI output removes backend-identifying fields.
Human diagnostics preserve the complete chain.

Errors are persisted in a diagnostics table with acknowledged/resolved state.
Transient UI notifications supplement the diagnostics inbox; they do not
replace it.

Optional absence is represented explicitly. Only a classified NotFound result
may become an empty collection. Mutex poisoning, serialization failures,
subprocess I/O failures, invalid deep links, window operations, event emission,
and optional provider failures cannot be discarded with `.ok()`, `let _`, an
empty fallback, or `if let Ok`.

## Desktop Information Architecture

Use persistent navigation:

1. Requests
2. Passwords
3. Connections
4. Agents & Models
5. Audit
6. Diagnostics
7. Settings

Requests combines pending, resolved, and automatic decisions with filters,
search, sort, server-side pagination, keyboard navigation, and a stable detail
pane. Duplicate global/per-request audit blocks are removed.

Passwords is the primary lightweight password-manager workspace. Its
three-pane interaction contains Vault/group/tag navigation, pageable Items,
and an Item editor with notes, sections, fields, history, and backend status.
It supports human create, edit, duplicate, move, archive, and delete. Enhanced
search, tag/key/note filters, aliases, and source information are integrated.
File and environment imports use the password-draft popup rather than one long
form.

Connections contains disabled and enabled external backends plus sync
adapters. 1Password and Bitwarden setup, scope, sync progress, refresh, health,
capabilities, and disable controls live here. LocalFolder, Git, WebDAV, and
custom HTTP sync configuration and conflict status are managed per Vault.

Agents & Models contains ACP presets, latest/pinned/custom controls, resolved
versions, provider configuration, and connection tests.

Audit is a standalone pageable and filterable history. Diagnostics shows daemon
status, protocol and package versions, backend sync state, persisted errors,
logs, retry actions, and copyable details. Settings becomes a full page grouped
by policy, prompts and memory, runtime, and locale.

The existing monochrome/red visual identity remains, with clearer hierarchy,
less duplicated chrome, consistent empty/loading/error states, visible focus,
keyboard operation, narrow-window layouts, and reduced-motion support.

The installed desktop application owns the background lifecycle on macOS and
Windows. Launching the app starts or attaches to `planktond`; closing the main
window hides it while the process, tray/menu-bar item, and daemon remain
available. CLI approval handoff reuses the running instance and focuses the
specific approval instead of cold-starting the desktop bundle. Explicit Quit
from the tray menu shuts down the desktop owner cleanly; daemon stop remains a
separate deliberate action.

The system icon is a flat radial plankton/keyhole mark. macOS uses a monochrome
Template Image so the system supplies the correct menu-bar foreground
(including pure white on a dark menu bar). Windows ships `.ico` sizes for the
tray and taskbar with equivalent light/dark contrast. Idle, attention,
reasoning, degraded, and disconnected states are distinct. ACP/API inference
sets the reasoning state; its tray mark rotates at a reduced frame rate, while
reduced-motion mode uses a static busy badge. State is daemon-derived so a
hidden window never leaves a stale icon.

## Migration and Cleanup

Configuration gains an explicit schema version and transactional migration.
The original file is backed up before the first write. Database migrations
remain additive and transactional.

Migration creates a KDBX4 Local Vault and a verified backup of the existing
catalog. Every legacy flat literal becomes a one-field Item and retains its old
resource ID as an alias. Imported records are grouped by provider
account/vault/item identity so their fields become one structured Item.
Descriptions become notes and tags remain attached. Migration validates every
resolved value before switching authority to the KDBX vault and is resumable
after failure.

The resource search index is derived data and can always be rebuilt. KDBX
vaults, backend bindings, request, approval, and audit data remain
authoritative.

Remove:

- obsolete `apps/desktop/src/main.ts`;
- duplicated ACP defaults;
- unused CLI status/report types and functions;
- desktop-local orchestration replaced by daemon calls;
- compatibility code proven unreachable after migration;
- muted-error fallbacks replaced by typed handling;
- the unused Bun lockfile after npm is made the sole frontend package manager.

Split oversized CLI, resolver, Tauri, app, hook, and password-management modules
by domain responsibility. This is structural cleanup, not a parallel legacy
path.

## Verification and Acceptance

Completion requires all of the following:

- formatter checks pass without modifying tracked files;
- Clippy passes for the entire workspace and all targets with warnings denied;
- all Rust workspace tests pass;
- frontend format, typecheck, tests, and production build pass on the pinned
  Node version without environment workarounds;
- daemon integration tests cover startup, authentication, restart recovery,
  cancellation, timeout, protocol mismatch, and persisted errors;
- KeePassXC engine tests cover manifest/hash verification, KDBX creation,
  unlock modes, CRUD, groups, custom fields, tags, history, atomic save,
  corruption handling, engine mismatch, cancellation, and merge;
- 1Password tests use a fake `op` executable and prove the read allowlist,
  AI write rejection, human-confirmed writes, source transparency, sync,
  partial failure, and get;
- Bitwarden tests provide equivalent read, human-write, transparency, sync, and
  failure coverage;
- password-add tests cover environment, `.env`, JSON, YAML, escaped dotted
  keys, JSON Pointer, structured selection, non-scalars, missing files, parse
  errors, persistence, popup confirmation, replay rejection, and destinations;
- sync tests cover local folders, Git divergence, WebDAV/HTTP ETags, offline
  retry, base/local/remote preservation, KDBX merge, invalid remote blobs, and
  the invariant that plaintext and unlock keys are never uploaded;
- search tests cover notes, field keys, tags, Unicode, CJK, typos, ranking,
  filters, pagination, stale cursors, merged backends, and partial results;
- ACP tests cover all presets, latest, pinned, custom migration, stderr, exit,
  and connection testing;
- UI tests cover Vault/group/Item/Section/Field management, history, sync,
  password-add popup, navigation, filters, pagination, setup-disabled state,
  diagnostics, errors, keyboard focus, and responsive layouts;
- macOS desktop smoke testing covers daemon connection and an approval round
  trip;
- CI runs on pull requests, `main`, and `dev`, covers all workspace crates and
  the frontend, and release jobs depend on verification;
- documentation describes the trusted-local boundary, KeePassXC/GPL component,
  daemon lifecycle, provider-neutral AI API, optional 1Password/Bitwarden
  setup, password-add flow, search syntax, sync adapters and conflicts, ACP
  version modes, diagnostics, migration, and recovery;
- a final requirement-by-requirement audit finds no remaining muted errors,
  shipped obsolete entry point, duplicated ACP default, or unverified explicit
  requirement.
