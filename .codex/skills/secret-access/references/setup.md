## Installation and startup

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

3. Human setup, vault unlocking and credential entry happen in the desktop app.
   Once connected, return to the credential workflow in [SKILL.md](../SKILL.md).
   Use existing task authorization and host permissions throughout.
