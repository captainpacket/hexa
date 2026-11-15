# ADR-2611151645: `hexa harden` checks its gate before it hunts, and keeps what the gate said

**Status:** Proposed
**Date:** 2026-11-15
**Drivers:** Two `hexa harden` runs on kairos (`src/usecases/worker.rs`, then `src/domain/passkey.rs`) spent 10m41s and 15m36s of inference on 10 confirmed-real findings. Every fix "did not pass the gate" and was reverted, and the final gate, run on the reverted tree, failed too, so the gate was red whatever the fixes did. Nothing says why: `run_evidence` returns the gate's output, and `harden` discards it (`let (passed, _) = …`). The same gate passes in 15 seconds on a clean `main` afterwards, so what made it red at the time is unknown. A run that cannot say whether its gate was ever green cannot judge a fix.

## Context

- **`harden` runs the gate only after it has tried a fix.** The hunt, the skeptical verification and the fix attempts come first. A gate that is red before the pass starts costs the whole pass, and every finding is left unfixed under the message "did not pass the gate — reverted", which reads as a verdict on each fix.
- **The output of a gate is thrown away.** `crate::direct_exec::run_evidence(gate, root)` returns `(passed, output)`. The fix loop and the final gate both bind the output to `_`. The loop's `last_run` records only `{at, gate, stage}`; the run record has an outcome and no log. After a failed run the reason had to be found by running the gate again by hand. This is the existing gap `harden-drops-gate-output`.
- **The preflight already blocks before the first hunt** when no inference path is reachable (`first_blocker`, `hexa harden` and `hexa build --harden`). A red gate is the same kind of fact: it is known before the pass starts, and reporting it three fixes late wastes the run.
- **A flake is not a red tree.** `hexa harden` already re-runs a full gate that failed and reports `flakes` (ADR-2610160756 §5). The same care applies to the first gate run.

## Decision

1. **The gate runs once on the tree as it is, before the hunt.** The full gate, not the fast one. A green baseline is recorded and the pass goes on.
2. **A red baseline is run once more.** Green the second time: a flake, noted in `flakes`, and the pass goes on. Red twice: the pass stops. No lens is asked, nothing is verified, no fix is tried, so no inference is spent. The report says the gate is red before any fix, carries the gate's output, and `hexa harden` exits non-zero.
3. **`--no-baseline` skips decision 1.** For a gate that is red on purpose (a test written for the bug, before the fix). It is the operator's word, and the report says the baseline was skipped.
4. **Every gate run keeps what it said.** A `GateRun` holds its label (`baseline`, a fix's title, `final`, `without any fix`), whether it passed, its duration, and the last 60 lines of its output (bounded at 8 KiB). The report lists them in order. A failing gate prints the last 12 lines of its output where it fails, and `hexa harden` prints the full 60 for a failing baseline or final gate.
5. **A second failed fix in a row asks whether the tree is red without any fix.** After the second consecutive fix that failed the gate, the gate runs on the reverted tree. Red: the fix loop stops, the remaining confirmed findings are listed as not judged, and the report says the gate is red without any fix, with its output. Green: the fixes really failed, and the loop goes on. This is checked on the unbatched path; the batched path (a fast gate) is covered by decisions 1 and 2, and by its own full gates (ADR-2610160756).
6. **The result goes to the run record.** `~/.hexa/runs/<id>.json` gains `gate_result: {passed, secs, tail}` for the pass's last gate run, so `hexa status` and the next session can say why a pass failed without running it again.

## Consequences

- A pass whose gate was red costs one gate run, not 10 to 16 minutes of inference, and says why.
- A green pass costs one more gate run than before. A gate that takes minutes costs minutes more; `--no-baseline` is the way out, and the cost is named here.
- A finding that was confirmed real and could not be judged because the gate was red is reported as that, not as a fix that failed.
- What this will NOT do: find why a gate was red at the time. It makes the next red gate say so, with its output.

## Not decided here

- Running `harden` in its own worktree, so a shared checkout cannot change under it (existing gaps `harden-resets-head-and-sweeps-staged-work`, `harden-deletes-new-untracked-files`).
- Running the project's formatter on a fix before gating it, and committing with the repository's own author instead of `hexa-factory`.
- Writing the confirmed findings to a file whatever the fix outcome.
- The same mid-run check on the batched path.

## Gate

`cargo test -p hexa-exec --test harden_gate_first`: at least 8 tests, written before the code. The frontier is a fake; the gates are shell commands with a counter file outside the tree. The oracle is hand-written: literal counts of gate runs and of calls to the frontier, and literal output lines. It covers: a red gate stops the pass with zero frontier calls and carries its output; a gate that fails once and then passes is a flake and the pass goes on; the output keeps its last 60 lines and not the first; a failing fix's note carries the gate's output; two failed fixes in a row on a tree that is red without them stop the loop; two failed fixes on a tree that is green without them do not; `baseline: false` runs no baseline; a green pass costs exactly one more gate run.

## Implementation

| Phase | Work | Exit gate |
|---|---|---|
| H1. Baseline and kept output | `GateRun`, the baseline phase, `gate_runs`, the output in notes. | `cargo test -p hexa-exec --test harden_gate_first` |
| H2. The CLI | `--no-baseline`, the report lines, `gate_result` in the run record. | `hexa dev ci` and `hexa dev validate` |

## References

- ADR-2610160756: the fast gate, batching, and flakes
- ADR-2609131646: a pass that reviewed nothing is not a pass
- The gap records `harden-drops-gate-output` and `harden-spends-the-run-before-checking-the-gate-is-green`
