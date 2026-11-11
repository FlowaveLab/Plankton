## Execute a bound consumer command

This requires Plankton 0.2.4 / protocol 6 or later with a matching desktop app
and CLI. Upgrade both together within the authorized installation flow.
`get`, its `request` alias and raw-value access/status APIs are disabled, even
for Direct fields and historical approvals: a declaration alone cannot bind a
consumer. Catalog metadata and exposure policies remain available.

Pass the real command and its literal arguments after `--`. With `--env NAME`,
the daemon injects the approved credential into that consumer environment
variable. Supply only a POSIX variable name, never `NAME=VALUE`. Runtime-control
variables such as PATH, BASH_ENV, ENV, PYTHONPATH, NODE_OPTIONS, LD_* and DYLD_*
are not credential destinations. The CLI never receives the credential value.

```bash
plankton run secret/api-token --env API_TOKEN \
  --reason "Query api.example.com status without logging or persisting credentials" \
  --requested-by agent -- python3 ./api-client.py --status
```

When the same consumer requires several fields, name the first with `--env`
and repeat `--bind RESOURCE=ENV` for each additional resource:

```bash
plankton run SECRET_ID --env CLS_SECRET_ID --bind SECRET_KEY=CLS_SECRET_KEY \
  --reason "Authenticate the CLS SearchLog request to cls.tencentcloudapi.com" \
  -- /usr/bin/python3 ./query_cls.py
```

Plankton reviews each field against its own policy and starts one consumer only
after every field is approved. Older versions without `--bind` need a matching
CLI and desktop upgrade. When automatic review fails, report the CLI handoff
message to the user while the same execution waits for their desktop decision.

The consumer reads `os.environ["API_TOKEN"]` (or the equivalent API in its
language), performs the authorized task and prints only non-secret results.
Without `--env`, the daemon sends the credential to consumer stdin; use that
mode for a consumer that reads its credential there. Keep all credential values
out of command arguments, source, caller input, output and model context.

Named scripts, recognized inline-code forms (such as Python `-c` and Node `-e`),
and shell `-c` wrappers can be inspected. A wrapper, environment injection or
heredoc is not itself a violation. The daemon identifies the runtime, source,
static child consumers and relevant file evidence; it binds what will actually
run at those recognized entrypoints. If a command has no recognizer or its consumer cannot be bound, stop and
report the unsupported part. Human approval cannot bypass failed admission.
This does not promise that every shell expression, native binary or dynamic
import is supported.

Use non-login, non-interactive shells; `zsh` requires `-f` to disable user
startup files. Use `--cwd` instead of shell `cd`, literal script paths instead
of `~` or variable-selected entrypoints, and simple environment references
such as `$API_TOKEN` or `${API_TOKEN}`. Complex shell expansion and output
redirection onto a reviewed entrypoint are refused. System runtime startup,
native-tool configuration, installed imports and dynamically selected behavior
remain trusted dependencies; the command evidence does not freeze all of them.

When the program itself reads code from stdin (for example `python3 -`, `node -`
or `bash -`), use `--env` so credential delivery does not compete for that same
stdin. Plankton captures the caller's code before submitting it for review:

```bash
plankton run secret/api-token --env API_TOKEN \
  --reason "Query the verified API status and return only the HTTP status" \
  -- python3 - <<'PY'
import os
import urllib.request
request = urllib.request.Request(
    "https://api.example.com/status",
    headers={"Authorization": "Bearer " + os.environ["API_TOKEN"]},
)
with urllib.request.urlopen(request) as response:
    print(response.status)
PY
```

Replace example resource IDs and hosts with ones verified for the task. The
quoted heredoc prevents the caller's shell from expanding its contents.
For a consumer that needs business input, use `--input FILE` or `--input -` with
`--env`; the daemon identifies whether that input is code or task data. It is
not automatically labeled as source evidence. Caller input must be UTF-8, at
most 64 KiB, and must not contain credentials. Stdin capture rejects an
interactive terminal and times out after five seconds if the input never ends.
Without `--input`, only recognized stdin-code consumers cause a caller-stdin
read; ordinary commands do not read it implicitly.

`--script FILE [--python PATH] -- ARGS...` remains a Python-file convenience
form; `/usr/bin/python3` is its default interpreter. It is not a restriction on
the command form. `--cwd PATH` sets the consumer directory; relative command
paths resolve there. Use an explicit interpreter path for a virtual environment.
The daemon records the recognized execution and rechecks bound file identities
before launch. Keep these files unchanged while approval is pending.

Each `run` submits once and polls the same execution. Human review opens the
desktop flow when required. Interrupting the CLI does not cancel an execution
already started by the daemon; do not blindly resubmit after a disconnect.
`--timeout` bounds consumer execution to 1–300 seconds (default 60), excluding
review and credential-read time. External credential-backend commands have a
separate 60-second deadline and 2 MiB limit per output stream.

This binds the consumer and credential handoff; it is not a sandbox. Installed
dependencies, dynamic I/O and concurrent changes by another process under the
same OS account are not fully isolated. Missing decision-relevant evidence must
not be replaced with an assertion that a wrapper or filename is safe.

Each consumer output stream is limited to 64 KiB; exceeding it stops execution
and discards output. Filter large responses in the consumer. Literal credential
output is blocked unless both terminal_log and llm_context allow it; short
values may match ordinary text, and this is not an encoding/splitting detector.
Never rely on that guard to make a consumer that echoes credentials acceptable.
Text mode forwards consumer stdout/stderr and exit status. JSON/JSONL return
execution status and output, never a raw credential value field. Denial,
admission failure and execution failure exit nonzero.
