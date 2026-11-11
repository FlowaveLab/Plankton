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
