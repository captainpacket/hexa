# ADR-2611151030: `hexa adr board` shows which decisions are done, how far each is, and who is on it

**Status:** Proposed
**Date:** 2026-11-15
**Drivers:** Kairos and Strata are built by several agents and people at once, in several checkouts (a main checkout, git worktrees, a second repository). Two things went wrong together. (1) Work collided: two sessions edited the same ADR or the same files, and nobody could see it until the merge. (2) hexa could not say what was finished. In kairos, `hexa adr list` shows `proposed` for ADRs whose phases are built and tested (ADR-2611121443 P5, ADR-2611130723), and `accepted` for ADRs with no sign they were built; `hexa adr gates` reports 11 of 14 recorded gates as failed because it cannot read `cargo test ... | grep -q` output, so it cannot be used as a progress number either. The owner asked for one panel: every ADR, proposed or in progress, with a progress figure, so "when are we done" has an answer.

## Context

- **Progress is written in prose, in a convention that already exists.** Kairos ADRs carry `## Phases`, each phase a heading `### R1. <title>`, and mark a built phase with `**Built 2026-11-14.**` in its section, or list it on the header line `**Built:** R1, R2 and R4a on 2026-11-14.` Non-phased ADRs carry `**Built:** F1 on 2026-11-14.` A `### Decision gate (R1, ...)` heading sits beside the phase headings and is not a phase. Nothing reads any of this.
- **`**Status:**` records a decision, not a build.** `Accepted` means the owner agreed; it does not say the code exists. Reading it as progress is what makes `hexa adr list` look wrong.
- **There is already a record of who is working.** ADR-2611051850 built `.hexa/tasks.json` (`hexa agent register|list|report|release`) with an owner, a worktree and leases per task. Nothing joins it to an ADR.
- **Each checkout has its own copy of `docs/adrs/`.** A branch in a worktree can accept an ADR, add phases or write a new ADR that the main checkout cannot see. `git worktree list` knows where they are.
- **hexa has no daemon and no dashboard** (founding goals). The panel is a command that reads files and exits, and, on request, writes one static page.

## Decision

1. **`hexa adr board [--json] [--text] [--html FILE] [--also DIR]... [--product DIR]... [--strict]`.** Read-only, except that `--html` writes `FILE`. Output is JSON when stdout is not a terminal or with `--json`, a table with `--text` or on a terminal, and a self-contained page with `--html`. Errors are JSON objects `{ok:false, code, message, hint}` on stdout, exit 1.
2. **A unit of work is a phase, or the ADR when it has none.** An ADR with a `## Phases` section has one unit per heading in that section that matches `### <id>.` where `<id>` is one to three letters and a number, optionally followed by a lowercase letter (`R1`, `P2a`, `F1`). Any other heading is not a unit. An ADR with no such heading is one unit.
3. **A unit is built only on evidence, never on status.**
   - A phase is built when its section contains a line that begins `**Built`, or when the ADR's header `**Built:**` line names its id as a whole token (`P2` does not name `P2a`).
   - A phase-less ADR is built when it has a `**Built:**` header line or its status is `Completed`.
   - A `Completed` ADR has every unit built, by declaration.
   - `Accepted` and `Proposed` count for nothing.
4. **Each ADR has a state, computed from its status and its units.**
   - `built`: every unit built.
   - `building`: at least one unit built, not all.
   - `accepted`: status Accepted, no unit built. It says "decided, no evidence of a build", which is not "not started" and not "done".
   - `proposed`: status Proposed, no unit built.
   - `retired`: Superseded, Rejected, Deprecated or Abandoned.
   - `unknown`: the status line cannot be read. It is listed and named in a warning.
5. **The summary is units, over the current checkout, and it rounds down.** `units`, `built`, `percent = floor(100 * built / units)`, over ADRs that are not `retired` or `unknown`, so 100 means done. It also gives `by_state` counts. `retired` and `unknown` ADRs count in `by_state` and nowhere else.
6. **Drift is stated.** An ADR whose status is Proposed and whose state is `building` or `built` is `status-behind-evidence`. `--strict` exits 1 when any ADR has drift, so CI can hold the ledger to the code.
7. **Other checkouts of this repository are read in place.** The board reads `docs/adrs/` of every worktree in `git worktree list` and of every `--also DIR`. Working-tree files are read, committed or not, because uncommitted work is the work that collides. For each ADR, each other checkout whose copy differs from the current one gets an `elsewhere` entry: the path, the branch, its status, state and `built/total`, and two facts about the difference.
   - `kind`: `status` when the status word differs, `progress` when the units built differ, `text` when only the text differs. The first one that applies.
   - `uncommitted`: whether that checkout has uncommitted changes to that ADR file (`git status --porcelain`). A checkout that is merely behind has `uncommitted: false`; a checkout with edits in flight has `true`. Only the second is a collision, so the table notes name `text` differences only when they are uncommitted, and name `status` and `progress` differences always. The JSON carries every entry.
   An ADR that exists only in another checkout is listed with `in_current: false` and is left out of the summary. A checkout that cannot be read is a warning, not an absent checkout.
8. **The ledger is joined, not copied.** A task in `.hexa/tasks.json` that is `claimed` or `running` is attached to an ADR when its name, gate or any lease contains the ADR's full id or its ten-digit number. The row shows each task's name, owner and status as `claimed_by`.
9. **Gate presence is shown, gate results are not run.** Each row says whether the ADR records a gate (`gate: recorded|none`). Running gates is `hexa adr gates`.
10. **Honest failure.** No `docs/adrs/`, or none with an ADR in it, is exit 1 `no_adrs`, never a board of zero. A warning never turns into a silent omission.
11. **Another product is its own board.** `--product DIR` reads a different repository's ADRs (kairos and Strata are two products with two ADR sets) as a separate board with its own summary, its own worktrees and its own ledger. Its rows never mix with this repository's, and its progress is never added to this one's. `--also` is for another checkout of the same repository, and a directory given to it that shares no ADR with this one is a warning that says to use `--product`.
12. **The page is static and inert.** `--html` writes one file with inline CSS, no script and no external reference. Every value taken from an ADR is escaped.

## Consequences

- Two sessions can see before they start that an ADR is claimed, and that another worktree has already changed it.
- "How far are we" has one number, from evidence. It will read low at first: kairos's older Accepted ADRs have no `**Built:**` line and stay `accepted`. That is the board being truthful, and the fix is a one-line edit in each ADR, not a change to the board.
- The progress convention becomes load-bearing. An ADR that does not follow it reads as not built.
- Cost: one read of each checkout's ADR directory and one `git worktree list` per run.
- What this will NOT do: it does not decide an ADR is complete (`hexa adr complete` does, gated on a workplan); it does not run a gate; it does not stop two agents editing one file (leases do, ADR-2611051850); it cannot see a checkout nobody listed with `--also` and no worktree names.

## Not decided / not built here

- **Gate results on the board.** `hexa adr gates` cannot read a gate that ends `| grep -q "test result: ok\. [1-9]..."`: grep prints nothing, so the runner calls the output unrecognised. Such a gate has its own zero-test guard, so reading it needs a decision about which pipelines are trusted. Until then the board shows gate presence only.
- **A per-phase gate.** Phases name a gate in prose; the board does not parse it.

## Gate

`bash scripts/gate-adr-board.sh`: it builds hexa from this tree and runs that binary, never the one on PATH, in a temporary git repo with a second worktree and a task in the ledger. Each check is named and printed PASS or FAIL; the script exits 0 only when every check passed and at least one ran.

## Implementation

| Phase | Work | Exit gate |
|---|---|---|
| B1. The rules and the board | `hexa-core/src/domain/adr_board.rs`: units, evidence, state, drift, summary, as pure functions with their own tests. `hexa adr board` for one checkout, `--json`, `--text`, `--strict`, honest failure. | `bash scripts/gate-adr-board.sh --phase 1` |
| B2. Other checkouts, the ledger, other products | Worktrees and `--also`, `elsewhere` with `kind` and `uncommitted`, `in_current`, `claimed_by`, `--product`. | `bash scripts/gate-adr-board.sh --phase 2` |
| B3. The page | `--html FILE`, escaped, inert. | `bash scripts/gate-adr-board.sh` (all checks) |

## References

- ADR-2611051850: the task ledger and leases this joins to
- ADR-2610271110: the unbuilt list; ADR-2609132122: `hexa adr gates`
- kairos `docs/adrs/ADR-2611141124` and `ADR-2611131030`: the `## Phases` and `**Built**` convention the board reads
