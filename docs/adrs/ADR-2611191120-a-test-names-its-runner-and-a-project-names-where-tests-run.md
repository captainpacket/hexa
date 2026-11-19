# ADR-2611191120: A test names its runner, and a project names where its tests run

**Status:** Accepted
> 2026-11-19 Accepted: Accepted under the owner's standing instruction (gary: always accept the ADRs and specs, and land the work); recorded by the implementing agent on his behalf, 2026-11-19
**Date:** 2026-11-19
**Drivers:** Running strata (Rust and TypeScript) through hexa's lifecycle on 2026-11-19. Its console is tested in vitest and its gates are bash scripts, and it builds only on its build servers.
**Supersedes:** none. Amends ADR-2611161600 §2 (the Cargo.toml refusal at `spec draft` and `spec accept`) and ADR-2611081151 §5 (the runners the evidence reader knows).

## Context

Two gaps stop a project that is not pure Rust, or that does not build on the machine that runs hexa, from closing its work.

**1. A scenario can bind only a Rust test.** `test_ref` in `hexa-core/src/domain/spec.rs` accepts only `<path>.rs::<fn>`. `run_test_alone` in `hexa-exec/src/spec_author.rs` runs only `cargo test`. In strata, behaviour tested in vitest or in a bash gate script can be drafted and accepted as a spec, but its scenarios stay `pending`, so `hexa spec verify` never passes. Three strata ADRs are blocked this way: ADR-2611182200 (37 scenarios), ADR-2611181933, and the continuous-UI-testing ADR (37 scenarios, bash and node gates). The evidence reader cannot read vitest either, so strata's builders recorded `--uncounted` evidence that greps a pass count. That is the "exit status alone" the reader exists to refuse.

**2. The lifecycle verbs run tests on the machine that calls them.** `hexa spec verify`, `hexa spec verify --prove`, `hexa spec red` (a checkout of the red commit, then a build) and `hexa loop stage done` (which reruns the recorded gate) all compile and run locally. So does `hexa do gate`, and `land` when a rebase moves the branch. strata builds only on its build servers: the developer Mac ran out of disk, and builds go through `ci/remote-gate.sh`, which clones a pushed branch on the less-loaded server. On 2026-11-19, `hexa spec verify` silently built a 2.4 GB target on the Mac.

As of v26.11.13, `hexa loop task done` does not rerun a gate. It checks off a step, and checks that the work is on main for the close-out steps. It runs nothing, so it needs no runner.

**The spec ledger needs no migration.** `Scenario.test` is a free string (`docs/specs/<ADR>.json`). Only `test_ref` and the code that runs and seals tests read it as Rust. A ref like `console/src/a.test.ts::adds two` or `ci/check-x.sh` is stored as is, and every existing ref keeps its meaning.

## Decision

1. **A test reference names its runner by its file's extension.** A ref is `<file>::<name>`, split at the first `::`; a `.sh` file may omit `::<name>`. The rules live in `hexa-core/src/domain/test_runner.rs` (`parse_test_ref`, `runner_for`):
   - `<path>.rs::<fn>` stays as it is: cargo, unchanged.
   - `<path>.{ts,tsx,js,mjs,cjs,mts,cts,jsx}::<test name>` runs vitest from the nearest directory at or above the file that holds a `package.json`: `npx vitest run <file> -t <pattern> --reporter=json`. The run passes only if exactly one test ran, it passed, and its full name or its title is the named one. It fails if no test ran or the one it matched was skipped (vacuous), if more than one ran (ambiguous), or if the JSON report is missing.
   - `<path>.sh` or `<path>.sh::<argument>` runs `bash <path> [<argument>]` from the repository root. It passes on exit 0, unless its output comes from a test runner hexa reads and reports zero tests passed: that run is vacuous.
   - Any other extension, with no runner configured for it, is refused by name: "no runner for `.x`", and the refusal names `test.runners`.

2. **A project adds or overrides runners in `.hexa/project.json`** under `test.runners`: an extension, without its dot, mapped to a command template. `{file}` is the path from the repository root and `{name}` is the part after `::` (empty if there is none). Each value is single-quoted for the shell when it is substituted. The command runs from the repository root and its output is read by the evidence reader (ADR-2611081151). It passes only if exactly one test passed and none failed. Zero is vacuous, more than one is ambiguous, and output the reader does not know is a failure, never a pass. A configured runner wins over a built-in one for the same extension.

3. **A project can say where its tests run.** `test.remote` in `.hexa/project.json` holds a command template with two placeholders. `{commit}` becomes the full 40-character commit id. `{cmd}` becomes the command to run in a checkout of that commit, at its root, single-quoted as one shell word. Example: `bash ci/remote-gate.sh --commit {commit} --no-gate --extra {cmd}`. A template that lacks either placeholder is refused when it is read. The template runs locally with bash from the repository root. Its exit status is the command's, and the evidence reader reads its output as it would a local run's. The rules are `remote_command` and `runner_ran` in `hexa-core/src/domain/test_runner.rs`.

4. **When `test.remote` is set, every verb that runs a test or a gate sends it through the template, for the commit concerned:**
   - `hexa spec verify` sends each scenario's test for HEAD.
   - `hexa spec verify --prove` and `hexa spec red` send each test for its red commit.
   - `hexa loop stage done` sends the recorded gate for HEAD.
   - `land`'s re-run after a rebase sends the gate for the rebased commit.
   
   All tests for one commit go in **one** remote run. hexa sends a script that prints a begin and an end marker, holding a per-run nonce, around each test's command, with the command's exit status in the end marker. It then judges each test from its own segment. One clone and one build serve every scenario of the commit.

5. **The remote run must prove itself, and it never falls back to a local run.** hexa refuses before running anything, naming the step to take, when:
   - the commit is on no remote-tracking ref ("push it": `git push`);
   - HEAD is the commit and tracked files differ from it (a remote run would describe a different tree: commit and push);
   - a test was proved red by copying it into an older tree (`--red-at`), which a remote checkout cannot reproduce.
   
   After the run, the output must name the full commit id, and every test's end marker must be present. Otherwise the run is a **runner failure**, reported as such: unreachable, or it did not run this commit. A runner failure is never a pass and never "red". `--local` on `spec verify`, `spec red`, `loop stage done` and `do gate` opts out, explicitly, for one run.

6. **`hexa do gate` under `test.remote` refuses unless `--local` is given.** It gates an uncommitted change, which no remote checkout can see, and it commits only after the gate passes. The refusal says to pass `--local`, or to commit, push, and close with `hexa loop stage done` or `hexa spec verify`, which send the gate remotely.

7. **The evidence reader counts vitest and a single bash script.** `read_gate_evidence` in `hexa-core/src/domain/evidence.rs` reads a gate command and its output:
   - vitest's summary line (`Tests  2 failed | 5 passed (7)`) and its JSON report (`numPassedTests`, `numFailedTests`) count as tests, like cargo's `test result:`.
   - A gate command that is exactly one bash script (`bash <path>.sh [args]`, `sh <path>.sh`, or `./<path>.sh`, with no `;`, `&&`, `||` or `|`) and exits 0 counts as one passed test, when its output holds no runner line the reader knows. When its output does hold such lines, they decide, so a script that ran zero tests is still vacuous.
   
   `do gate`, `loop stage done` and `land` read through it. `KNOWN_RUNNERS` names vitest and the bash script.

8. **A file bound to a scenario is a test file, so it may be sealed, when:**
   - it is under a `tests/`, `test/` or `__tests__/` directory;
   - its name has a `.test.` or `.spec.` part; or
   - it is a `.sh` script, since a gate script bound to a scenario is the test.
   
   Sealing and `hexa spec red --test` use this rule (`is_test_file`) in place of "under `tests/`". A source file is still refused, because sealing it would freeze the implementation.

9. **`spec draft` and `spec accept` refuse only where no test could be bound.** A project with a Cargo.toml, a package.json, or a `test.runners` entry passes. Otherwise the refusal keeps ADR-2611161600 §2's wording, naming Cargo.toml and that `hexa spec tests` generates Rust tests only. It adds that a test in another language is bound with `hexa spec red <ADR> <commit> --test <S>=<file>::<name>`. `hexa spec tests` still generates Rust only.

## The `.hexa/project.json` shape

```json
{
  "test": {
    "runners": { "py": "python3 -m pytest {file} -k {name} -q" },
    "remote": "bash ci/remote-gate.sh --commit {commit} --no-gate --extra {cmd}"
  }
}
```

The contract for a remote template:

- It receives the full commit id in place of `{commit}` and the command, single-quoted as one word, in place of `{cmd}`.
- It runs that command in a checkout of exactly that commit, at the checkout's root, with `bash -c`.
- It prints the full commit id it checked out, anywhere in its output.
- It exits with the command's status. A non-zero exit for the runner's own failure is fine, because the missing markers expose it.
- It never pushes, and it keeps nothing between runs that could make one commit's result depend on another's.

## Consequences

- strata's vitest and bash scenarios can be bound with `hexa spec red --test`, sealed, and verified. With `test.remote` set, its lifecycle verbs build on its servers and never on the Mac.
- A remote run costs one clone and one build per commit, not one per scenario.
- `--local` is the only way back to a local run, and it is typed each time.
- `hexa spec tests` still generates Rust tests only. A TypeScript or bash scenario's test is written by a person or a builder, before the code, and bound with `hexa spec red --test` after hexa has watched it fail at its commit.
- `hexa spec tests` itself (its red check) still builds locally. It is not routed through `test.remote` here; that is recorded as a follow-up gap.
