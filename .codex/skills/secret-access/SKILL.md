---
name: secret-access
description: Discover credentials and execute an authorized consumer through Plankton without exposing values. Use when a task needs credentials or credential catalog changes.
---

# Secret Access

Use Plankton's provider-neutral CLI to find credentials and inject them into the
actual consumer executed by the daemon. The CLI returns the consumer's result,
never a raw credential. Keep values out of model context, source, arguments and
output. Raw `get` and raw-value APIs are disabled.

## Use an existing credential

1. Reuse the installed CLI; if absent or unreachable, read [setup](references/setup.md).
2. Find metadata with `plankton search <service-or-field>`. Resolve an ambiguous
   account or service before choosing a resource.
3. Prepare the consumer to read credentials from environment variables or stdin
   and return the authorized task result.
4. Submit one `run` with every credential needed by that consumer; wait for that
   execution and continue the original task when it succeeds.

```bash
plankton run RESOURCE_A --env CLIENT_ID \
  --bind RESOURCE_B=CLIENT_SECRET \
  --reason "Authenticate the authorized request to api.example.com" \
  -- python3 ./client.py
```

Replace resource IDs and destinations with verified task-specific values.
`--bind RESOURCE=ENV` adds a credential to the same command; omit it for one
credential. `ENV` is a variable name, never its value. Each field retains its own
policy; the consumer starts once, after all fields pass. This does not cover
future independent calls or unknown dynamic credential use.

Without `--env`, the credential goes to consumer stdin. If stdin carries code or
business input, use `--env` and the input forms in [execution](references/execution.md).
`--cwd` sets the consumer directory; `--timeout` limits consumer runtime, not
approval waiting. Keep bound files unchanged while approval is pending.

## Follow the returned state

Plankton may approve, deny, or wait for a desktop decision. Surface a human
handoff while waiting on the same execution. A denial is final for that request;
do not weaken policy or change identity to retry. Interrupting the CLI does not
cancel an already-started consumer: check its outcome before resubmitting.

`run` binds the actual consumer; it is not a sandbox and does not freeze dynamic
dependencies. Human approval cannot enable unsupported consumer modes.

## Read details when needed

Use these references, or `plankton skill show TOPIC` outside the skill directory:

- `setup`: [installation, startup and compatibility](references/setup.md).
- `credentials`: [missing values, imports and exposure policies](references/credentials.md).
  For related missing keys, open one `password create` draft with known
  destinations; the human supplies values in the app. Saving does not approve execution.
- `execution`: [inline code, stdin, wrappers, pipelines and limits](references/execution.md).
- `catalog`: [metadata changes and their confirmation](references/catalog.md).
  Reuse `--change-id` to aggregate a pending change.
- `boundaries`: [value handling and authorization edge cases](references/boundaries.md).

Use existing task authorization and host permissions; the skill grants no extra
installation, account, credential or approval rights. Do not fetch a credential
merely to test access or ask for its value in chat.
