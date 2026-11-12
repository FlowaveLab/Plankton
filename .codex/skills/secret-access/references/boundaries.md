## Value handling and authorization

- Management changes can update item IDs, titles, notes, tags, resource keys,
  membership and retained locators, or delete entries. Their request and response
  schemas cannot carry secret values: `edit`, `rename-field`, `edit-field`,
  `refresh`, `move-field`, `merge`, `dedupe-field`, `delete` and `change` have no
  `--value` option. These stage daemon changes; they are not direct password-manager writes.
- Never put values in chat, summaries, source, arguments, caller input, fixtures,
  logs, screenshots or terminal transcripts. For missing values use one
  `password create` draft per related group; the human enters them in the app.
- `password add --env NAME` imports a selected existing variable. Its value is
  not request metadata. Do not use command substitution to turn a secret read
  into a CLI argument.
- Use `run` to bind and execute the real consumer. Declaring `--consumer-script`
  metadata alone, piping a disabled raw `get`, or printing/encoding the value
  through a nominal consumer does not provide that binding.
- A trusted computer does not grant permission to bypass required confirmation.
  Creation confirmation and one-time execution approval do not expand saved
  policy. Do not omit known destinations or switch to `direct` / unrestricted
  network access just to suppress approvals; see [credentials](credentials.md).
- If no supported consumer mode can accomplish the task without revealing the
  credential, explain the limitation. Do not request raw-value access.
