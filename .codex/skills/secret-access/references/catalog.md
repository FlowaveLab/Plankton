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
