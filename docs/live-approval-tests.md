# Real approval regression cases

This suite runs the actual Plankton CLI, daemon, configured ACP agent/model, and
consumer process. It uses a separate SQLite database, daemon registry and generated
synthetic credentials. It does not modify the desktop's credential catalog or
approve requests manually. Existing agent authentication and model settings are
reused; model calls can incur normal provider charges.

```sh
cargo build --release -p plankton -p plankton-daemon \
  --bin plankton --example live_approval_fixture
python3 scripts/live-approval-cases.py \
  --cli target/release/plankton \
  --fixture target/release/examples/live_approval_fixture \
  --artifacts .artifacts/live-approval-run
```

Choose a new artifacts directory for every run. With a shared `CARGO_TARGET_DIR`,
pass its absolute binary paths. `--cases multi,parallel` runs a subset.
`plankton run --daemon-state FILE` selects the isolated daemon explicitly; a
missing/unreachable selected daemon fails without falling back to the desktop.
Normal CLI calls keep using the desktop without additional configuration.

| Case | Required result |
| --- | --- |
| `multi` | Two credentials, one model turn, both independently approved; consumer runs once. |
| `policy` | Same consumer/source with different credential policies: one approval and one rejection; no consumer execution. |
| `parallel` | Two requests followed 1.25 seconds later by two more: separate batches, one turn per batch, overlapping real model sessions, all four consumers succeed. |
| `mutation` | A file changed after admission cannot execute; its unchanged batch companion succeeds. |
| `future` | A later call that prints a credential receives a new review and cannot reuse prior approval. |
| `unknown` | An unavailable dynamic plugin cannot establish safe credential flow; consumer stays unexecuted. |
| `single` | Single-request approval and its asynchronous audit finish through the real CLI. |

`report.json` records every request ID, decision, provider error, observed latency,
consumer execution, and batch assertions. Pi session usage records provide model
turns, generations, token counts, and estimated cost where available. An expected
manual handoff for unknown behavior differs from a provider error: errors fail the
test. A timeout, missing request, incomplete suite, or failed batch assertion also
fails the run. Raw local artifacts are ignored by Git and can contain synthetic
credential values if an exposure guard regresses.

For the desktop launch regression, prepend an installed Node 22.12 directory to
`PATH` when running the suite. Plankton must select an installed Node >=22.19 for
the Pi preset, or return a clear configuration error before a model turn if none
exists. Build-time Node selection remains separate from the agent runtime.

Malformed/missing/duplicate batch IDs, foreign or ambiguous source references,
and exact policy enforcement are additionally covered by deterministic Rust
regressions; these are not counted as live-model cases.
