## Bootstrap and first use

When this skill is loaded and the task needs a credential, continue through the
steps below within the user's existing authorization and the host agent's tool
permissions. Do not stop at recommending commands when you can execute them.
This skill does not grant additional installation, account, or approval rights.

1. Check `command -v plankton`. Reuse an installed CLI; do not reinstall it on
   every invocation. If missing, check `uname -sm` and `command -v brew`.
   The packaged app currently supports Apple Silicon macOS. If Homebrew is
   available and installation is authorized, install the app and CLI together:

   ```bash
   brew install --cask flowavelab/tap/plankton
   plankton --version
   ```

   If the machine is unsupported, Homebrew is absent, or the host blocks the
   install, state that concrete blocker. Do not disable Gatekeeper, remove
   quarantine, install unrelated tools, or weaken host permissions to proceed.
   This already-loaded skill does not need to reinstall itself with `npx`.

2. Start the installed desktop app on macOS:

   ```bash
   open -a Plankton
   ```

   Bare `plankton` prints CLI help; it does **not** start the desktop daemon.
   Use a targeted `plankton search <service-or-field-hint>` to check readiness
   and discover metadata. If startup is still in progress, wait briefly and
   retry once. If connection still fails, report the failure; do not print
   `daemon.json`, which contains a local bearer token, or dump runtime logs.

3. Choose the matching resource from metadata. Prefer a targeted search over
   listing the entire catalog. If it is ambiguous, resolve the service/account
   with the user. If missing, import only an explicitly selected source or use
   `password create` so the human can enter the missing values in the app.

4. Request only the resource the task needs with `plankton run`. Pass the actual
   consuming command after `--`, identify its purpose and destination in
   `--reason`, and choose stdin or `--env NAME` credential injection. The daemon
   identifies the consumer and binds its inspectable source before executing it.
   Replace example resource IDs and hostnames with ones verified for this task.
   Never obtain a secret just to test access.

5. Human setup, vault unlocking, credential entry, and human approval still
   happen in the desktop UI. Existing configured policies may approve, deny,
   or escalate access. A denial is final for that request: do not retry with
   weaker policy settings or a different identity. Report only the operation's
   outcome, never the secret. Resume the user's original task after approval.
