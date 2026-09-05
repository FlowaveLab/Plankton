# Plankton Desktop Approval and UI Design

**Date:** 2026-07-30

## Goal

Make automatic approval completely silent, surface only requests that genuinely
need a human, choose a compact or full approval surface from the current main
window focus state, replace the obsolete Codex ACP adapter, and finish the
desktop console as one coherent product without changing its black, paper, and
vermilion identity.

The installed `/Applications/Plankton.app` and `/opt/homebrew/bin/plankton`
must be rebuilt and exercised after the change.

## Product Boundary

Plankton is a trusted-computer local approval console and lightweight password
manager. Automatic evaluation is ordinary background work, not a notification
event. A window is justified only when a human decision is required.

Password values may be persisted by Plankton or an enabled external password
manager, but approval, audit, diagnostics, handoff URLs, and model prompts must
remain value-free. The compact approval window shows the resource identifier
and decision context, never a resolved secret.

## Approval State

`human_review_required` is derived centrally from policy, approval state, and
evaluation state:

| Policy | Evaluation | Approval | Human review |
| --- | --- | --- | --- |
| manual | not required | pending | yes |
| assisted | queued or running | pending | no |
| assisted | completed, failed, or interrupted | pending | yes |
| automatic | queued or running | pending | no |
| automatic | completed with an automatic terminal decision | approved/denied | no |
| automatic | completed without a terminal decision | pending | yes |
| automatic | failed or interrupted | pending | yes |
| any | any | approved/denied | no |

The daemon returns this value in resource access and status responses. It is a
read-model field, not a second source of truth. The CLI opens the desktop only
on the first transition from `false` to `true`; queued/running automatic work
continues to poll silently.

The persistent desktop process also watches pending requests so API and ACP
callers receive the same behavior as CLI callers. Duplicate deep links and
polling observations are collapsed by request ID.

## Window Selection

The main window starts hidden when Plankton is launched as a background
process. Explicit tray or application launches show the main window.

When a request becomes human-reviewable:

- if the main window is visible, focused, and not minimized, navigate it to the
  full Requests detail without changing its size;
- otherwise create or reuse one `approval` window, sized about `500 × 620`,
  non-resizable, and show the compact review;
- if more requests arrive, keep one window and show a queue count; resolving a
  request advances to the next;
- closing the compact window leaves the request pending;
- “Open full details” closes the compact surface and opens the main Requests
  detail;
- resolving the last queued request closes the compact surface.

The compact surface contains resource, requester, reason, context, risk and
evaluation summary, optional human note, Approve, Reject, and Open full details.
It is a distinct React route selected from the Tauri window label, not a
miniaturized copy of the full workspace.

## ACP Runtime

The Codex preset changes from the archived
`@zed-industries/codex-acp` package to
`@agentclientprotocol/codex-acp`.

- Latest resolves to `npx -y @agentclientprotocol/codex-acp@latest`.
- Pinned resolves to the same package with the selected version.
- Custom programs remain untouched.
- Legacy `@zed-industries/codex-acp@latest` and the historical default are
  migrated to the new Latest preset.
- A pinned legacy Zed version remains an explicit deprecated custom command;
  its version is never reinterpreted as a version of the new package.

Connection diagnostics distinguish configured selector, actual adapter
identity/version, and the runtime reported by the agent. The existing
initialize probe remains the fast basic check. A readiness check additionally
creates a session and sends a minimal no-tool prompt so model/runtime
incompatibility is found before an approval request depends on it. Errors are
shown in the Agents page and diagnostics instead of being muted.

## Visual Direction

The product remains a hard-edged local security console:

- ink `#171716`;
- paper `#F4F1EA`;
- surface `#FFFEFB`;
- vermilion `#F2381E`;
- rule `#CFCAC1`;
- muted ink `#706D67`.

Georgia is reserved for page titles. The system sans stack is used for UI text,
and the system monospace stack is used for resource IDs, versions, JSON, and
commands. Spacing follows `4, 8, 12, 16, 24, 32`. Controls have square or
two-pixel corners, visible focus rings, and at least a 36-pixel hit target.

The signature element is an approval-state rail: a thin, legible sequence from
request to automatic evaluation to human decision. It makes the security
workflow memorable without adding decorative animation. The organism logo
remains static; the existing dedicated spinner represents background
evaluation.

All icons use named `lucide-react` imports at 16 or 18 pixels with
`strokeWidth={1.75}`. The Plankton brand remains a custom mark. Text buttons
hide decorative icons from accessibility APIs; icon-only buttons have an
accessible name.

## Shared UI Structure

`DesktopWorkspace` owns navigation and a scrollable content viewport. Global
styles are reduced to application bootstrap and legacy surfaces. All workspace
selectors are scoped under `.desktop-workspace` to eliminate the current
collision between `styles.css` and `workspace.css`.

Page primitives provide:

- page heading, description, status and primary action;
- toolbar with search/filter controls;
- full-height split pane;
- empty, loading, and error states with a next action;
- bottom-anchored pagination rendered only for more than one page;
- dialog focus trap, Escape, scroll lock, sticky header/footer, and focus
  restoration.

## Page Behavior

### Requests

The list/detail layout fills the available height. When no request is selected,
the detail empty state occupies the complete right pane. When no requests
match, the list and detail collapse into one guided empty state. Status filters
separate awaiting human, evaluating, completed, and failed. Detail sections
show summary, approval-state rail, context, model rationale, note, and actions.

### Passwords

The vault tree, list, and detail columns use stable heights and independent
scroll regions. Search covers title, notes, field key, and tags. Tag match mode
uses the same 40-pixel control height as other inputs. A field row separates
label/metadata, masked or revealed value, and Eye/Copy actions. The detail
action area remains at the bottom. Pagination appears only when needed and
stays at the list bottom. Narrow layouts expose filters in a drawer rather than
hiding them.

The existing add/import flow keeps the required destination confirmation. Its
header and footer remain visible while its body scrolls.

### Connections

Password backends and encrypted sync destinations are separate groups.
Existing connections use responsive rows with aligned health and actions.
Adding a sync destination opens a focused drawer/dialog instead of occupying a
permanent empty grid cell.

### Agents and Models

The ACP configuration is a centered, readable form with a runtime status panel.
Latest, pinned, and custom modes remain explicit. Custom command fields are
advanced controls. Basic and readiness checks report their distinct results,
including actual adapter/runtime versions and errors.

### Audit

Audit becomes a full-width event list with actor, action, result, and time
filters. Selecting an event opens its details without narrowing the whole page
to a 780-pixel column. An empty audit is a real empty state, never a fake
numbered record.

### Diagnostics

A daemon status strip precedes a full-width error list. Severity and
acknowledgement filters are available, full error text is preserved, and the
healthy state is explicit.

### Settings

Settings are edited directly on the Settings page. A left category rail
switches General, Approval, Providers, ACP, Passwords, and Sync sections. The
form body scrolls while its Save bar stays visible. The obsolete “Open
configuration” action and settings modal are removed.

## Responsiveness and Accessibility

The layouts are checked at `1440×900`, `1024×768`, `820×740`, and `620×800`.
At narrow widths, navigation and secondary filters become drawers; no
capability disappears. Long resource IDs, commands, JSON, and diagnostics
wrap or scroll within their own bounds without clipping the page.

Every interaction is keyboard reachable. Focus is visible, dialogs trap and
restore focus, Escape closes transient surfaces, and state never relies on
color alone. `prefers-reduced-motion` disables nonessential motion and selects
the static tray activity frame.

## Error Handling

Background evaluation errors are persisted and shown on their request and in
Diagnostics. Window presentation failures are logged and retained as
diagnostics without changing the approval decision. ACP startup, basic probe,
readiness probe, and version compatibility failures have distinct messages.
Frontend command failures render inline alerts; no `.catch(() => undefined)`
or empty collection converts a failure into apparent success.

## Verification

Automated coverage proves the complete approval matrix, silent automatic
evaluation, a single human-review transition, focus-based surface selection,
compact queue behavior, legacy ACP migration, new package resolution,
readiness incompatibility reporting, scoped styles, empty/error/overflow
states, conditional pagination, settings inline editing, icons, focus behavior,
and long bilingual copy.

Installed-runtime verification must prove:

1. an automatic approval completes without showing or focusing a window;
2. a human request with the main window unfocused opens only the compact
   window;
3. a human request with the main window focused opens the full Requests detail;
4. a real Codex ACP readiness prompt succeeds through the new adapter;
5. all seven pages render at desktop and narrow sizes without clipped controls;
6. the rebuilt app and CLI report the expected version and daemon health.
