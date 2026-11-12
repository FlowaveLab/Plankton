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
| `single` | One admitted credential returns decision and complete source evidence in one model turn through the real CLI. |

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

## Frozen business-input replay

Build `cargo build --release -p plankton-daemon --example replay_approval_snapshots`.
The example accepts an input JSON path and an output JSON path. Input is an array
of batches, each containing `BatchReviewInput` objects (`request_id`, `policy_mode`,
`input`). Use the stored sanitized `provider_input_json` snapshot for `input` and
preserve original batch membership. Run from a private artifacts directory using
the same model settings as the baseline. This reviews evidence only: it neither
loads credential values nor executes the consumers. Tools may still inspect files
needed by the model, so snapshots with changed external dependencies need separate
interpretation.

Compare per-request decisions, all five exposure states/levels, destinations,
validated source citations, errors and session usage. A matching decision alone
is insufficient. Report model generations separately from ACP prompt turns. For
cost comparisons, apply the same published prices to both runs; the Pi cost field
is an estimate and may not account for provider time-of-day discounts.

## 2026-11-12 business replay (0.2.8)

Ten frozen business batches (16 credentials), using DeepSeek V4 Pro with thinking
off, returned the same decisions and all five exposure levels/states as the 0.2.7
baseline. All 16 retained complete validated source evidence; provider errors: zero.
ACP prompt turns fell from 16 to 10, model generations from 58 to 37, total input
(including cache reads) from 657,906 to 360,924 tokens, and output from 40,971 to
25,406 tokens. Median model-session duration, including the old audit phase, fell
from 47.0 to 29.2 seconds. At identical published peak prices, estimated cost fell
from CNY 2.1821 to 1.6236 (25.6%); this is not an account billing export. These
replays reviewed captured business evidence without executing business consumers.
Model/tool counts remain workload-dependent; no hard tool-call limit was added.

The final real CLI suite also passed all 11 scenarios (13 credential requests):
independent mixed policies, overlapping batches, source mutation rejection, fresh
review for later calls, unknown-dependency handoff without execution, and complete
single-request evidence in one ACP turn.
