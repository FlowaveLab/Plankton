## Boundaries

- Management commands may change item IDs, titles, notes, tags, resource keys,
  field membership, retained locators, or delete entries, but their request and
  response schemas cannot carry or return a secret value.
- Supply `--reason` when starting a change. Additional operations using the same
  pending `--change-id` inherit that reason and appear in one cumulative diff.
- Never put credential values into chat, summaries, code, arguments, fixtures,
  logs, screenshots, or terminal transcripts. Use `password create` for missing
  values so the human enters them directly in the app.
- `password add --env NAME` names an existing environment variable to import;
  it is not an instruction to expose that value in request metadata.
- If a task cannot use the supported consumer mode without revealing its
  credential, explain that limitation rather than requesting raw-value access.

## Common mistakes

- Retrying disabled `get`, declaring an unrelated consumer, or using a script that simply prints/encodes its credential.
- Asking the user to paste a missing secret into chat instead of using `password create` for human entry.
- Creating one empty draft per key when the keys belong to the same credential group; use one `password create` call with repeated `--key` options.
- Treating confirmation of `password create` as approval for `run`, or attempting to bypass the normal access request after creation.
- Leaving a known API destination out of the creation draft, or using `direct` / unrestricted network access to suppress repeated approvals.
- Piping an unbound raw `get` result into a consumer, or treating `--consumer-script` metadata as execution binding; use `run` so the daemon inspects and executes the actual command.
- Treating a trusted computer as permission to bypass desktop confirmation.
- Passing a value to a management command. There is deliberately no `--value`
  option on `edit`, `rename-field`, `refresh`, `move-field`, `merge`,
  `dedupe-field`, `delete`, or `change`.
- Describing management as direct password-manager writes. It stages a
  metadata-only daemon change and waits for human confirmation.
