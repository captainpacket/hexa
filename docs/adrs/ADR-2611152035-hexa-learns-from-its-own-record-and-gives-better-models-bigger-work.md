# ADR-2611152035: hexa learns from its own record, and gives better models bigger work

**Status:** Accepted
> 2026-11-20 Accepted: Accepted in bulk on 2026-11-20 at Gary Berger's instruction ("accept all adrs"); not individually reviewed by the agent
**Date:** 2026-11-15
**Drivers:** The operator, watching `hexa harden` on a downstream project (harness, `apps/lintel/renderer`): "Is hexa learning from its mistakes or is it coding and then fixing and coding and then fixing?" Then: "As the models get better, we need hexa to also transform and take advantage more and more of the models. That means hexa has to learn."

## Context

One run shows the problem. On 2026-11-15, `hexa harden apps/lintel/renderer` (hexa 26.11.11, frontier delegate, gate about 30 s with an e2e step) took 19.5 minutes:

| Phase | Time | What happened |
|---|---|---|
| hunt | 1 min | 4 fixed lenses, 9 candidates |
| verify | 40 s | 9 of 9 confirmed real |
| fix | about 17.5 min | 9 fixes, **one at a time**, the full gate after each |
| result | | `0 fixed`: all 9 gate-passed but left uncommitted, because they touched files with the operator's uncommitted work |

What it did not do:

- **It did not name the cause the findings shared.** Six of the nine were one bug class: an async result applied without a guard against a newer call or a second click in flight. They were fixed as six separate guards. No rule was proposed for `.hexa/ADR-rules.toml`, no lesson was stored in `hexa memory`, and no shared helper was made. The next change, by a person or an agent, starts without the knowledge. The operator's own UI change in the same session was written that way.
- **It could not be told what to look for.** The lenses are fixed. Once the class was known, the obvious next pass ("find every unguarded async apply in these files and fix them with one helper") had no verb. `hexa do run` comes closest: one file, one instruction, one evidence command.
- **It worked in the operator's tree.** When its fix agent committed, harden reset the branch to the commit it started from. That also dropped a commit the operator made during the run. `hexa do run` on the same day committed the whole dirty tree, not just `--file`.
- **It treated a frontier model like a weak one.** The serial, one-fix-at-a-time loop with a full gate per fix assumes each fix is a gamble that must be isolated. Measured on this run, every one of the nine fixes passed its gate first time.

ADR-2610221834 already designs the fix for the third point and half of the fourth: parallel worktrees per fix, test first and shown red, landing in a fixed order, never touching the operator's tree. It is still Proposed. ADR-2610281516 already measures what inference costs. ADR-2609211200 gives memory a project scope. The pieces for learning exist; nothing closes the loop.

## Decision

hexa keeps what each run teaches, uses it on the next run, and sizes the work it hands a model by what that model has been measured to do. The gate never gets weaker: it is the one thing that does not scale with model quality.

1. **Build ADR-2610221834 first.** Harden fixes in parallel worktrees, test first, red then green, and never reads or writes the operator's working tree or branch. Every later item assumes fixes are isolated.

2. **Findings are clustered by cause before they are fixed.** After verify, a frontier call groups the confirmed findings by root cause (one call, all findings, the code they cite). A cluster of two or more gets **one fix**: the model is asked for the shared change (a helper, a type, a wrapper) and a regression test per finding. The cluster is gated as one unit. A singleton is fixed as today.

3. **Every cluster leaves something behind that outlives the run.** For each cluster, harden writes, in this order of preference:
   - a **rule** in `.hexa/ADR-rules.toml` when a pattern can find the class. The rule is proposed with the run id as its incident and checked against the tree **before and after** the fix: it must fire on the old code and stay quiet on the new. A rule that fires on fixed code is dropped, as the removed fallback rule was.
   - otherwise a **lesson** in `hexa memory` under `lesson:<class>`, and a line for CLAUDE.md's "lessons a rule cannot catch". The line is proposed, not written into the managed block.
   - and a **class test** where one can be written: a test over every call site of the class, so a new site that skips the shared fix fails.

   The run's report lists what it left behind. A run that fixed a cluster and left nothing reports it as a gap.

4. **What hexa learned goes into what it asks next.** Before a hunt, a `do run`, a `build` or a generated test, hexa puts the project's `lesson:*` entries and the rules that touch the target paths into the prompt, ordered and cached per ADR-2610281516 §3. A lens that a lesson already covers becomes a targeted check ("verify these N call sites follow lesson:async-staleness") rather than an open hunt.

5. **Harden takes a focus.** `hexa harden --focus "<what to look for>" <target>...` adds a lens written by the operator or taken from a lesson (`--focus lesson:async-staleness`). With a focus and no other lens, harden runs that one check across all targets. This is "give the model what we are looking for and the files", with verify and the gate kept.

6. **The size of the work follows the measured model, not a constant.** hexa keeps, per model and per verb, its record from the spend log and the run reports: findings confirmed versus refuted, fixes that passed the gate first time, regression tests that were really red, retries. From that record:
   - **batch size**: a model whose fixes pass first time on this project gets larger batches under `--fast-gate`, up to the whole pool. A model that fails gets smaller ones, down to one.
   - **lens choice**: lenses that found nothing real on this project in the last N runs are dropped from the default set, and the model is asked to propose the lenses for a target from the lessons and the code.
   - **unit of work**: `do run` allows more than one file when the model's record supports it, still under one evidence command.

   The record and the decision it produced are printed in the run's opening lines, so the operator can see why a run was sized the way it was. `hexa bench agentic` remains the cold start for a model with no record.

7. **The gate, the verify step and the red-test rule never scale down.** A better model gets bigger units of work, not fewer checks. Refute-by-default verify, a regression test that must be shown red, and a full gate before anything lands stay as they are for every model. A model's record can change how much it is given, never whether it is checked.

## Consequences

- A second run on the same code costs less and finds the class at once: the lesson becomes a focused check, and the shared fix becomes the pattern to follow.
- The rule file grows from incidents automatically. The before/after check keeps it honest, but it needs review: a proposed rule is a change to the project's policy, and goes through the same acceptance as any other edit to `.hexa/ADR-rules.toml`.
- Sizing work from a model's record means two projects can see the same model handled differently. That is intended: the record is per project, because a model's strength depends on the language and the code (see the bench note on Rust and Go).
- Prompts get longer as lessons accumulate. Lessons are scoped to the target paths and capped. A lesson that no run has used in N runs is reported as stale, not deleted.
- The clustering call is one more frontier request per run. It is cheap next to the fixes it merges: six fixes became one on the run in Context.

## Implementation

**Gate:** `cargo test -p hexa-exec --test harden_reads_lessons` covers §4: the hunt, fix and build prompts carry the lessons that bear on the target, and a fix is told to fix the whole class. It grows as the other items land.

| Phase | Description | Gate |
|---|---|---|
| P1 | ADR-2610221834 built: worktrees, test-first, red-green landing, operator's tree untouched | its own tests; a run with a dirty operator tree leaves that tree and branch byte-identical |
| P2 | Clustering after verify; one fix per cluster | a fixture with 3 findings of one cause and 1 other yields 2 fix units |
| P3 | Rule / lesson / class test written per cluster, with the before-and-after rule check | a proposed rule fires on the pre-fix tree and not the post-fix tree, or is dropped |
| P4 | Lessons and matching rules in the hunt, do, build and spec prompts | a lesson stored in the project appears in the next hunt prompt for a matching path |
| P5 | `harden --focus` (text or `lesson:<key>`) | a focused run hunts only the focus across all targets |
| P6 | Per-model, per-verb record; batch size, lens set and unit size from it, printed in the opening lines | a model with N first-time passes gets a larger batch than one with N failures, on the same fixture |

## References

- ADR-2610221834: harden fixes in parallel worktrees and proves each regression test red (P1 builds it)
- ADR-2610160756: harden gates each fix fast and each batch in full
- ADR-2610281516: inference is measured, cached and repaired, not repeated (the spend log, prompt ordering)
- ADR-2609211200: memory belongs to the project it was learned in
- ADR-2611151645: harden checks its gate before it hunts
- ADR-2610301500: a dead end gives a guide and records the gap
- Gaps recorded in harness on 2026-11-15: `harden-does-not-generalize`, `harden-focus`, `harden-resets-others-commits`, `do-run-commits-whole-tree`, `doctor-skills`
