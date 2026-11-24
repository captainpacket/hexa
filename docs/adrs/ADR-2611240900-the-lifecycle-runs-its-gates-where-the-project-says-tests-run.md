# ADR-2611240900: The lifecycle runs its gates where the project says tests run

**Status:** Accepted
> 2026-11-24 Accepted: the owner asked for it (gary, 2026-11-24, on the four gaps datum found: "should we fix hexa?" … "Yes, all four"). Recorded by the implementing agent on his behalf. The spec is drafted, not accepted: `hexa spec accept ADR-2611240900` is his.
**Date:** 2026-11-24
**Drivers:** datum, 2026-11-24. datum sets `test.remote` and its owner forbids builds on the Mac. ADR-2611191120 §4 and ADR-2611221610 §7 send `spec verify`, `spec red`, `loop stage done` and `do gate` to the remote runner. Four parts of the lifecycle still assumed the gate runs here: `hexa harden` (gaberger/hexa#74), the evidence reader (it could not read cargo-nextest, the runner datum's gate uses, or coloured output), a hexa verb run inside the remote runner's own clone, and `loop stage done`'s evidence and its idea of "pushed".
**Supersedes:** none. Amends ADR-2611191120 §4 (harden joins the verbs sent to the remote; the evidence command goes with the gate) and §5 (the "pushed" check reads the remote; a run on the runner is not sent again), ADR-2611081151 §5 (the runners the reader knows) and ADR-2611231400 §1 (the counts it reports). It decides new things with their own scenarios, so it is a new ADR rather than an addendum: an addendum records what a build of the same decisions settled.

## Context

- **harden.** `run_review_gated` applies each fix to the tree and runs `--gate` here through `run_evidence`. So do the baseline, the red-first check, the "red without any fix" check, every `--fast-gate` run, every batch's full gate and the final gate. Under `test.remote` each of those is a local build. datum's builders could not harden at all without breaking the owner's rule.
- **The evidence reader.** `cargo xtask gate` in datum runs cargo-nextest. Its output is read as follows (real samples, `cargo-nextest 0.9.152`, kept in `hexa-core/src/domain/evidence_samples/`). A green run has no line the reader knows (`Summary [ … ] 2 tests run: 2 passed, 2 skipped`), so it is `Unrecognised` and refused. A red run is read only through the libtest output nextest captures for the failing test (`test result: FAILED. 0 passed; 1 failed`), so its passes read as zero. Colour codes hide every line: a coloured vitest run that passed (`ESC[2m      Tests ESC[22m ESC[1mESC[32m2 passed…`) was refused on 2026-11-24, because no line starts with `Tests `.
- **A verb inside the remote clone.** The runner checks the commit out and runs the command there. When the command is itself hexa (a loop gate of `hexa spec verify X`, sent by `stage done`; or `ci/remote-gate.sh … --extra 'hexa spec verify X'` by hand), that hexa reads the same `test.remote` and sends its tests to the runner again. On datum the nested run cannot reach the runner, and every scenario reads "not run: runner failure". Where it can reach it, it costs a second clone and a second build of the same commit.
- **`loop stage done`.** It sends the gate through `test.remote` for HEAD (ADR-2611191120 §4). But "pushed" is answered from the local remote-tracking refs (`refs/remotes/*`), which say what the remote held at the last fetch, not what it holds now. And the recorded evidence command (`hexa loop evidence`) runs here with `sh -c`, a local build when it is a test command. `loop task done` runs no gate at all (it checks a step off, and checks that the work is on main for the close-out steps). That is kept and pinned by a test.

## Decision

1. **`hexa harden` under `test.remote` gates on the remote runner.** Every gate run of a pass runs on the remote runner. That covers the baseline, each fix's gate (or its `--fast-gate`), the red-first check, the "red without any fix" check, every batch's full gate, the bisection and the final gate. Each run gates the tree as it stands at that moment:
   - hexa builds a scratch candidate commit, HEAD's tree plus the pass's changes, in a throwaway index, so the branch and the index do not move;
   - it pushes the candidate to `refs/heads/hexa-gate/<first 12 of the candidate>` on the branch's remote (its upstream's remote, else `origin`);
   - it runs the gate through the template for that commit, judges the output as a local run's is judged, and deletes the scratch ref, on every path.
   
   The candidate carries every tracked change. Of the untracked files, it carries only those the pass created: an untracked file that was in the tree when the pass began is the operator's, and it is not pushed. The fix loop is otherwise unchanged. A fix is committed on the local branch only after its gate passed on the runner. A failing fix is reverted, and its scratch ref is gone either way. A runner failure (the output does not name the candidate, or a marker is lost; ADR-2611191120 §5) is a failed gate: the fix is not kept, it is never "red first", and the note says it was a runner failure. A repository with no remote to push to is refused before the hunt, naming `git remote add` and `--local`. harden still does not push the branch: `hexa do gate` or a plain push does that. `--local` runs every gate here, as before. The rule is `run_tree_remote` in `hexa-exec/src/test_exec.rs`. The pass is told where to gate by `run_review_gated_at(…, GateAt::Remote(template), …)` in `hexa-exec/src/adversarial.rs`; `HardenGates` is unchanged, so every caller that builds one is too. `hexa build --harden` keeps its gates local. It is not a verb datum's builders use, and its build gate runs here too.

2. **The evidence reader reads cargo-nextest, and strips colour first.** `hexa-core/src/domain/evidence.rs`:
   - **ANSI.** Every reader (`read_evidence`, `count_tests`, `first_failure`, `read_red_evidence`) removes ANSI escape sequences (CSI `ESC [ … final`, OSC `ESC ] … BEL` or `ESC \`, and two-byte `ESC x`) before it reads a line. Output with colour reads exactly as the same output without it.
   - **nextest's summary** `Summary [ <time> ] N tests run: P passed[ (k flaky)], F failed, T timed out, S skipped` counts P passed. The failures are F plus T plus any `exec failed`. Skipped tests are not counted. The cancelled form `n/N tests run:` is read the same way.
   - **`Starting N tests across …`** names the runner, as `running N tests` does, so `Starting 0 tests` with nothing else is vacuous. So is nextest's `error: no tests to run`.
   - **When a summary is present it is the count,** and the libtest lines nextest prints inside a failing test's captured output (`running 1 test`, `test x ... FAILED`, `test result: FAILED. 0 passed; 1 failed`) are not counted again. A gate that runs nextest and another runner (vitest, go) sums both.
   - **Without a summary** (cut off), nextest's per-test lines count, each test name once: `PASS [ … ]` as a pass, and `FAIL`, `TIMEOUT`, `SIGSEGV`, `SIGABRT`, `SIGKILL` or `ABORT [ … ]` as a failure.
   
   `KNOWN_RUNNERS` names cargo-nextest.

3. **A hexa verb on the remote runner runs its tests there.** It knows it is on the runner by the environment variable `HEXA_ON_RUNNER`, holding the full id of the commit under test.
   - hexa's batch script (ADR-2611191120 §4) exports `HEXA_ON_RUNNER=<the commit it was sent for>` before each command, so a hexa that the command starts knows it is on the runner without the project's template doing anything.
   - A project's template may export it too, for commands it runs that hexa did not send. For datum's `ci/remote-gate.sh`, that is `export HEXA_ON_RUNNER="$(git rev-parse HEAD)"` before it runs `--extra`.
   - By hand: `--extra 'HEXA_ON_RUNNER=$(git rev-parse HEAD) hexa spec verify X'`, or `--local`, which has meant "run here" since ADR-2611191120 §5.
   
   `test_config` reads it, in a project that sets `test.remote` (elsewhere it changes nothing, so it is not read: a hexa that a runner's command starts in some other repository, such as a test fixture, is not refused for it). When it is set and equals HEAD of the checkout, `test.remote` is treated as unset for that process: `spec verify`, `spec red`, `loop stage done`, `harden` and `do gate` run their tests in this checkout. When it is set and names another commit, or a value that is not a 40-hex id, every verb that reads the test setting refuses, naming the variable, both ids and the fix. Then a runner that checked out something other than what it was sent is caught from the inside. The marker is not "the checkout is the commit under test" by itself: a developer's own checkout is also at HEAD, so the checkout alone cannot tell a runner from a Mac. The rule is `on_runner` in `hexa-core/src/domain/test_runner.rs`.

4. **`loop stage done` under `test.remote` asks the remote whether HEAD is pushed, and sends its evidence with the gate.**
   - Before anything runs, hexa reads the branch's upstream head from the remote itself (`git ls-remote <remote> <branch ref>`). HEAD is pushed when that head is HEAD, or a commit HEAD is an ancestor of. If HEAD is not pushed, the stage refuses with the code `head-not-pushed`, the hint `git push <remote> <branch>` and nothing sent. This covers a remote that has lost or never had the commit, though the local remote-tracking refs still name it. A branch with no upstream falls back to ADR-2611191120 §5's check (on some remote-tracking ref), and its refusal carries the same code.
   - The recorded evidence command goes to the runner in the same batch as the gate, one run for HEAD, and its segment of the output is what is appended to the ADR. When the gate fails, the evidence is not appended.
   - `--local` runs both here, as before.
   - `loop task done` runs no gate, here or remote, and that is unchanged: a step is checked off by the person or agent who did it, and the gate is `stage done`'s.

## Consequences

- A datum builder runs `hexa harden <path> --gate '<cmd>'` with no flag. Each fix's gate is one remote run. Nothing builds on the Mac, and the scratch refs are gone after the pass.
- `cargo nextest` gates are counted (`tests.passed` in `do gate`'s result object is the real number), and coloured output no longer needs `NO_COLOR` to be read.
- A loop gate of `hexa spec verify X` under `test.remote` costs one remote run, not two, and passes on the runner.
- `stage done` can no longer pass on a commit the remote does not have.
- **Not decided here:** the SubagentStop hook still reruns a subagent task's gate here (`close_subagent_task` in `hexa-cli/src/commands/hook/mod.rs`), and `hexa build`'s gates run here. Neither is a verb datum's builders use today. Both are recorded as workplan items.

**Gate:** `cargo test -p hexa-cli --test adr_2611240900_spec && cargo test -p hexa-exec --test harden_remote_gate && cargo test -p hexa-core --lib domain::evidence && cargo test -p hexa-core --lib domain::test_runner`
