# ADR-2611151330: `hexa dev build-keys` catches a native build that recompiles depending on the command

**Status:** Accepted
> 2026-11-20 Accepted: Accepted in bulk on 2026-11-20 at Gary Berger's instruction ("accept all adrs"); not individually reviewed by the agent
**Date:** 2026-11-15
**Drivers:** In Strata, DuckDB's C++ (about 295 MB of objects, minutes of CPU) was compiled three times into one target directory in under a day, with identical output each time. Several agents work in several worktrees, each running a different cargo command (`build -p strata-server`, `test -p strata-conformance`, `clippy --workspace --all-targets`), so each rebuilt what the others had built. Nothing in the project's gates could see it, and finding the cause took a long investigation. The cause is a property of the dependency graph, so a check over the graph, with no compilation, finds it in seconds.

## Context

- **A build script reruns when its own build changes, and a native one compiles C or C++ every time it reruns.** `cc` does not reuse object files. DuckDB's `libduckdb-sys` build script has 82 units in its dependency tree.
- **Cargo's choice of key depends on the command.** The unit graph (`cargo <cmd> --unit-graph`, nightly-only, usable on stable with `RUSTC_BOOTSTRAP=1`) lists every unit with its features and profile. Measured on Strata, the script's tree differed across commands in two ways:
  - *profile*: a crate used both as a build dependency and as a normal dependency is built once with the target's debuginfo when the command also builds a crate that needs it at target level, and with the build-override's debuginfo when it is host-only. 26 of 82 units differed between `--workspace` and `-p strata-store-duckdb`.
  - *features*: `syn` got `full`, `extra-traits`, `visit` and `fold` only when `strata-server` was selected. Build-dependency features unify per command, over the packages it selects.
- **The profile name is not a difference.** `dev` and `test` units with equal settings share artifacts (checked: `cargo test --no-run` after `cargo build` reports the dependency Fresh), so the name is left out of the key.
- **Two changes closed it in a scratch copy of Strata:** `[profile.dev.build-override] debug = true`, and `syn` with the union of its features as a *build*-dependency of a crate every selection includes (a normal dependency does not affect build-script features). All 12 selections then gave the script one input set. This was checked in the unit graph and has not been confirmed by a cold build.
- hexa's gates (`hexa dev ci`) check architecture, ADR rules, specs and the API contract. None looks at build keys.

## Decision

1. **`hexa dev build-keys [--selection ARGS]... [--json] [--text]`.** Read-only. It runs no compiler: it asks cargo for the unit graph of each selection and compares them. JSON when stdout is not a terminal or with `--json`.
2. **Selections.** A selection is a cargo argument string. Order of precedence: `--selection` (repeatable), then `.hexa/project.json → build_keys.selections`, then the default: `build`, `test --no-run` and `clippy --all-targets` over the workspace, plus, when the workspace has more than one member, `build -p M` and `test --no-run -p M` for each member `M`. A selection is split on whitespace; it has no quoting.
3. **Unit graph.** `RUSTC_BOOTSTRAP=1 cargo <selection> --locked --offline --unit-graph -Z unstable-options`, retried without `--offline`. A selection that yields no graph is an error that names it, never a skipped selection. Fewer than two graphs is an error: nothing can be compared.
4. **A native build script** is a `custom-build` unit in `build` mode whose dependency tree contains a package named `cc` or `cmake`. Only a selection that contains the script is compared for it.
5. **The key of a script** is its dependency tree: for each unit, its package id, target name and mode, its sorted features, and its profile settings except the name.
6. **A finding.** A native script with two or more distinct keys is `unstable`. It lists the groups of selections that share a key, and up to ten differing units, each with a `kind`: `features` (the sets differ), `profile` (the settings differ) or `presence` (a unit is in one tree and not the other).
7. **Hints by kind.** `profile`: make the build-override agree with the profile (`[profile.<p>.build-override] debug = ...`). `features`: pin the unified set where every selection sees it, as a build-dependency of a crate every selection includes, which is what a cargo-hakari `workspace-hack` crate does. The hint says it is a prediction from the unit graph.
8. **Exit codes.** 0: every native script is stable, or there is none (the report says `native_scripts: 0`). 1: at least one is unstable. 2: it could not check (no graph, one selection, not a cargo project).
9. **`hexa dev ci` runs it** when `.hexa/project.json` has `build_keys.enabled: true`, and prints one line. It does not run without that setting, so no project's `hexa dev ci` changes by default.

## Consequences

- The next change that makes a native build depend on the command fails a gate in seconds, instead of costing minutes of CPU in every agent's worktree.
- Cost: one `cargo --unit-graph` per selection. On Strata, 21 selections took 1.8 seconds.
- Measured on Strata with the first build of this verb: five native build scripts are unstable, not only DuckDB's. `libduckdb-sys` has 3 keys (profile and `syn` features); `libsqlite3-sys` and `ring` differ in `cc`'s `parallel` feature and in `jobserver`/`libc` presence; `psm` and `stacker` differ in `libc`'s debuginfo. `clippy --workspace --all-targets` is a key of its own for every one of them.
- What this will NOT do:
  - It predicts cargo's cache keys from the unit graph. It does not prove a rebuild does not happen; only a cold build and a second command do.
  - It only compares the selections it is given. A command nobody listed can still diverge.
  - It cannot see environment variables a build script declares (`CC`, `CXXFLAGS`), which also trigger a rerun.
  - `-Z unstable-options` is nightly-only; `RUSTC_BOOTSTRAP=1` is the escape hatch, and a cargo that drops `--unit-graph` makes the check fail loudly (exit 2), never pass.
  - It does not fix the instability; it names the unit and the knob.

## Not decided / not built here

- **Build admission.** A machine-wide limit on heavy cargo runs (load reached 57 on 32 cores). Separate ADR; it would build on the task ledger (ADR-2611051850).
- **Caching the C++ compile** (sccache). Untested: DuckDB's sources are extracted into the target directory, so include paths differ per worktree.
- **A shared target directory.** Rejected for now: it serialises builds on cargo's lock, and it leaked stale artifacts into a hexa test that builds its own crate.

## Gate

`bash scripts/gate-build-keys.sh`: it builds hexa from this tree and runs that binary, never the one on PATH, over small offline cargo workspaces with a `cc` build dependency. Each check is named and printed PASS or FAIL; the script exits 0 only when every check passed and at least one ran.

## Implementation

| Phase | Work | Exit gate |
|---|---|---|
| K1. The rules and the verb | `hexa-core/src/domain/build_keys.rs`: parse a unit graph, find native scripts, key them, compare, classify differences; pure, with tests on hand-made graphs. `hexa dev build-keys`. | `bash scripts/gate-build-keys.sh --phase 1` |
| K2. In `hexa dev ci` | The opt-in line (decision 9). | `bash scripts/gate-build-keys.sh` (all checks) |

## References

- ADR-2611051850: the task ledger a build-admission ADR would build on
- Strata's `cargo xtask gate` (the Strata repository's own gate, which runs these commands; its ADR is not in this repository)
- cargo: `--unit-graph`, resolver 2 and 3 feature unification, `[profile.*.build-override]`; cargo-hakari
