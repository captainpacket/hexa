# ADR-2611231400: A remote gate commits red or green, tries itself first, and says what it did in one object

**Status:** Accepted
> 2026-11-23 Accepted: the owner asked for it (gary, 2026-11-23: "build the hexa verb for remote gated commits"). Accepted under his standing instruction to accept the ADRs and specs and land the work, recorded by the implementing agent on his behalf
**Date:** 2026-11-23
**Drivers:** datum, 2026-11-23. Four datum builders could not commit source: the commit hook sent them to `hexa do gate`, and the hexa they had (26.11.15) still refused `do gate` under `test.remote` without `--local` (ADR-2611191120 §6). `--local` builds on the Mac, which datum's owner forbids. The lead committed around the hook by hand. ADR-2611221610 §7 (released in 26.11.16) already sends `do gate` to the remote runner. Four gaps are left, and this ADR decides them. The gate-first pipeline needs a **red** commit: the tests alone, failing for the right reason. `do gate` commits only on a pass. A remote gate cannot be tried before it pushes. Its result is prose that an agent has to parse. A branch push that the remote refused was a warning and exit 0, and no step read the remote head back.
**Supersedes:** none. Amends ADR-2611221610 §7 (the branch push and its read-back, and the result object) and §9 (the hook names `--expect-red`), and ADR-2611160730 §3 (`do gate` can commit a red).

## Context

ADR-2611221610 §7 builds a candidate commit of the named files on HEAD and pushes it to `refs/heads/hexa-gate/<first 12>`. It sends the gate through `test.remote` for that commit and requires the runner's output to name it. The branch moves only on a pass, and the scratch ref is deleted every time. That path stays. Four things are still missing:

- **No red commit.** CLAUDE.md's pipeline writes the tests first and commits them while they fail, so `hexa spec red <ADR> <sha>` can check out that commit and watch them fail. `hexa do gate` commits only on a pass, and the hook refuses a hand `git commit` of a test file. Under `test.remote`, then, a tests-first commit has no honest path. A gate that every honest path has to go around teaches agents to go around it (ADR-2611221610, Context).
- **No dry run.** Every write in a hexa project's agent surface can be tried first (datum's CLAUDE.md, primary goal). A remote `do gate` pushes a ref, runs a remote build, moves a branch and pushes it. None of that can be previewed.
- **No result an agent can read.** The outcome is human lines. A failure carries no machine code, hint or field, and it does not say whether a retry could help. "The runner did not run this commit" and "the gate failed" read alike.
- **A pass that did not reach the remote looks like a pass.** If the branch push was refused, `do gate` printed a warning and exited 0. Nothing read the remote head back, so "pushed" was the push's exit status. CLAUDE.md's definition of done rules that out.

## Decision

1. **`hexa do gate` ends with one result object.** The last line of its standard output is one JSON object, on success and on every failure, local or remote, dry run or not. The human lines before it stay. Its fields:
   - always: `ok` (bool), `mode` (`remote`, `local` or `dry-run`), `expect` (`green` or `red`), `files` (repo-relative paths), `branch`;
   - when a candidate was built: `commit`, its full id;
   - when the gate ran: `tests`, an object `{"passed": n, "failed": n}` counted by the evidence reader (`uncounted: true` in its place under `--uncounted`), and `first_failure`, the first failing line of the output, when there is one;
   - on a remote pass: `remote` (the remote's name) and `remote_head`, the id `git ls-remote` reads back for the branch after the push;
   - on a failure: `code`, `hint` (the next step to take), `field` (the flag the failure is about, or `null`) and `retryable` (bool: whether the same command, run again unchanged, could succeed).

   The codes, decided here:
   - `nothing-to-commit` (`--files`): a named file has no change.
   - `refused` (`--files`): a credential, a path outside the repository, or a broken seal.
   - `no-upstream` (`null`): the branch tracks no remote branch.
   - `push-refused` (`null`, retryable): the remote refused the scratch ref or the branch.
   - `remote-runner-wrong-commit` (`null`, retryable): the output does not name the candidate, or its batch markers are missing.
   - `remote-gate-failed` / `gate-failed` (`--evidence`): the gate ran on the commit and failed, remotely or here.
   - `vacuous-gate` (`--evidence`): zero tests ran.
   - `unrecognised-evidence` (`--evidence`): the output names no runner hexa reads, and no `--uncounted` was given.
   - `branch-moved` (`null`, retryable): the branch moved while the gate ran.
   - `remote-head-mismatch` (`null`, retryable): the read-back differs from the commit.
   - `not-red` (`--expect-red`): the gate passed.
   - `red-not-a-test-failure` (`--expect-red`): a build or compile error, or a failure no runner counted.

   The AG-UI run's `RUN_ERROR` code is the same code with `-` written `_` (`gate_failed`, as before). The rules for counting and for the first failure live in `hexa-core/src/domain/evidence.rs` (`count_tests`, `first_failure`).

2. **The scratch ref stays `refs/heads/hexa-gate/<first 12 of the candidate>`, and it is deleted on every path.** It is a branch because a runner clones a branch. datum's `ci/remote-gate.sh` fetches every branch and tag on git.local, then refuses a commit that none of them reaches. A plain `git clone` does not fetch a custom namespace such as `refs/hexa/…`, so a commit kept there would not reach the runner. The `hexa-gate/` prefix names who made the ref and why. The candidate's id makes the name unique and ties it to the commit it carries. The ref is deleted after the run on a pass, a failure and a runner failure alike. A project's CI should not build `hexa-gate/**`; datum's runs only on `main` and on pull requests, so it does not.

3. **A remote pass is done only when the remote says so.** After the branch moves to the candidate (ADR-2611221610 §7), hexa pushes it to its upstream, never forced. It then reads the upstream branch's head back with `git ls-remote <remote> <branch ref>`. The pass is `ok: true` only when that head equals the commit. A refused branch push is `push-refused`, and a differing read-back is `remote-head-mismatch`. Both exit non-zero and do not land. Either way the commit stays on the local branch, because it passed the gate. The hint names `git push <remote> <branch>` and says to rebase and gate again if the remote moved. Before this ADR, a refused push was a warning and exit 0.

4. **`--expect-red` commits the named files only if the gate fails as a red test fails.** The candidate is built, pushed and sent to the runner exactly as for a green gate (or run here under `--local`). The verdict is read by `read_red_evidence` in `hexa-core/src/domain/evidence.rs`, which returns one of:
   - `Red { passed, failed }`: the gate exited non-zero, the output holds no build or compile error, and a runner the reader knows counted at least one failed test. Only this commits.
   - `NotRed`: the gate passed. Refused with `not-red`: tests written first must fail before the code.
   - `BuildError`: the gate failed, and its output holds a build or compile error: rustc's `error[E…]`, `error: could not compile`, or `error: aborting due to`; TypeScript's `error TS…`; or vitest's `Failed to load` or `Transform failed`. Refused with `red-not-a-test-failure`, because a compile error is not a red test. This holds even when another suite counted failures, since a red that is half a compile error hides which tests are red.
   - `Uncounted`: the gate failed and no runner counted a failed test (a script's `exit 1`, a crash, nothing recognised). Refused with `red-not-a-test-failure`.
   - `Vacuous`: a runner the reader knows reported zero tests. Refused with `vacuous-gate`.

   A runner that did not run the commit is `remote-runner-wrong-commit`, as for a green gate. `--expect-red` cannot be combined with `--uncounted` (an exit status alone cannot tell a red test from a broken build). It implies `--no-land`: a red commit never lands on the base, because it would turn the base red. The commit message records `Gate: <cmd> (expected red: failed on test assertions; …)`. The counts are not in the message: the candidate is built before the run, and the commit the branch moves to is the commit that was tested. Under `test.remote` the counts are in the result object and in the `refs/notes/hexa-gate` note.

5. **A red commit names its next step, and does not record itself in a spec.** `hexa do gate --expect-red` does not write `docs/specs/<ADR>.json`. Which scenarios a red commit proves is the binding `hexa spec red` makes, with its `--test S=<file>::<name>` for each one, and `hexa spec red` already checks the commit out (remotely under `test.remote`) and watches each test fail there. A second writer of the same record would be a second source of truth. Instead the result object carries `next`: `hexa spec red <ADR> <full commit id>`. `<ADR>` is the loop's recorded ADR (`hexa loop adr`) when there is one, and the literal `<ADR>` otherwise. The same line is printed.

6. **`--dry-run` does nothing and says what it would do.** It runs every check `do gate` makes before it builds a candidate: paths in the repository, no credentials, each file changed, seals intact, and under `test.remote` an upstream for the branch. It then prints the plan and exits 0, or the refusal and exits non-zero. Nothing is pushed, run, committed, recorded in the loop or written to the run log. The plan is the result object with `mode: "dry-run"`, `ok: true` and a `would` object:
   - `parent`: HEAD's full id.
   - `commit_files`: the files.
   - `push`: the scratch ref and the remote.
   - `run`: the `test.remote` command, with `{commit}` shown as `<candidate>`.
   - `then`: the branch update and push, and whether it would land.

   Under `--local` the plan is the gate command, then the commit, then the landing.

7. **The commit hook names the red path too.** The hook's refusal of a hand `git commit` of source still names `hexa do gate --files … -e … -m …`. Under `test.remote` it still says the gate runs on the remote runner (ADR-2611221610 §9). In every project, with or without `test.remote`, it now adds that a tests-first commit (tests that must fail before the code) is the same command with `--expect-red`, and that `--dry-run` shows what either would do. A commit of prose only is still not refused, and so names nothing.

## Consequences

- A datum builder commits tests first with `hexa do gate --files <tests> -e '<gate>' -m 'test: …' --expect-red`, then the code with `hexa do gate --files <code> -e '<gate>' -m 'feat: …'`. Each is one remote run on the build servers, nothing builds on the Mac, and no commit goes around the hook.
- Every outcome is one JSON line an agent can branch on, by `code`, without reading prose.
- A refused branch push now fails the verb where it used to warn. A caller that relied on exit 0 after a refused push sees exit 1 and `push-refused`, which is what happened.
- `hexa spec red` still runs its own remote run of the red commit. A red commit costs two remote runs: one to commit it, and one to bind it. They are kept apart on purpose (decision 5).

**Gate:** `cargo test -p hexa-cli --test adr_2611231400_spec && cargo test -p hexa-core --lib domain::evidence`

## Addendum, 2026-11-23: what the build settled

- **One cargo failure is counted once.** The evidence reader counted each `test x ... FAILED` line as a failure and then the `test result:` summary's count again, so one failed test read as two. Only "failed > 0" was used before, so nothing broke. The counts the result object reports (§1) made it visible. Per-test lines (`... FAILED`, go's `--- FAIL`) now count only when no summary counted failures. Package-level markers (go's `FAIL\tpkg`, a bare `FAIL`) count as one failure when nothing else counted any. Whether a run fails is unchanged.
- **A single script that passes counts as one test** in the result object, as it does for the verdict (ADR-2611191120 §7), so `tests.passed` is never 0 on a pass.
- **The help names the flags.** `hexa do gate --help` gives an example of the red commit, the code commit and the dry run. `AGENTS.md`, the `hexa-workflow` and `hexa-git-effects` skills, and the `hexa-builder` agent name `--expect-red`, `--dry-run` and the result object. The `hexa-workflow` skill still said that `do gate` refuses under `test.remote` without `--local`, which has been false since ADR-2611221610 §7. That line is replaced.
- **The domain rules** are `count_tests`, `first_failure`, `read_red_evidence` and `RedEvidence` in `hexa-core/src/domain/evidence.rs`, re-exported by `hexa-core/src/ports/evidence.rs`. Judging a run for its expectation is `judge_gate` in `hexa-exec/src/direct_exec.rs`. The dry run's plan is `gate_plan` in `hexa-exec/src/test_exec.rs`.
