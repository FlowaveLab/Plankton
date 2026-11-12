## Manage catalog metadata

Use metadata-only commands with an audit reason. Changes require desktop
confirmation unless automatic mode explicitly permits every included change.
Exposure-policy changes always require human confirmation.

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
  confirmed version to finish. Operations appended to a pending `--change-id`
  inherit its reason. Query either mode explicitly:

```bash
plankton password change CHANGE_ID
plankton password change CHANGE_ID --wait --timeout 300
```

- To import selected values or ask the human to enter missing ones, use
  `password add` / `password create` as described in [credentials](credentials.md).
  These drafts always require human confirmation; values never return to the CLI.
- Password management commands never return secret values. They submit metadata-only operations and return a change ID, cumulative diff, state, and optional successor change ID.
- `move-field` and `merge` preserve resource keys while changing password-item membership. `dedupe-field` compares two existing stored values locally and refuses deletion unless they match; neither value is returned.
- A confirmed change ID is immutable. Calls that arrive afterward are accepted under a successor change ID in the same batch instead of being rejected.
- Every `plankton run` is a separate reviewed execution request. Saving a field or its exposure policy does not execute a pending consumer.
