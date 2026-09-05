# Bundled KeePassXC engine

Plankton's local Vault is a standard KDBX4 database. Plankton does not
implement KDBX cryptography; it invokes a pinned `keepassxc-cli` sidecar with
typed argument vectors and verifies the packaged artifact checksum before use.

The release pipeline extracts only the CLI and runtime files needed for each
target from the upstream artifacts recorded in `manifest.json`. Distribution
must include KeePassXC's GPL license, the exact corresponding source archive or
a valid source offer, upstream notices, and the extraction/build recipe.

Passwords are provided through the child process stdin. They must never appear
in argv, logs, diagnostics, or environment variables. Vault writes operate on
a private temporary copy, validate it, keep a backup, and atomically replace
the authoritative KDBX file.
