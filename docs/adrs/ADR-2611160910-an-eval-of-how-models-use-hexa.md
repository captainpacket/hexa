# ADR-2611160910: An eval of how models use hexa

**Status:** Accepted
> 2026-11-20 Accepted: Accepted in bulk on 2026-11-20 at Gary Berger's instruction ("accept all adrs"); not individually reviewed by the agent
**Date:** 2026-11-16
**Drivers:** The audit behind ADR-2611160730 found that agents used about 8% of hexa, and that guidance alone (CLAUDE.md, skills) changed nothing. ADR-2611160730 changes the hooks, the verbs and the guidance. Nothing measures whether those changes move a model's behaviour, so every claim that they do is a guess.
**Supersedes:** none. Measures ADR-2611160730.

## Context

- The evidence so far is one surviving working transcript, read by hand: 33 of 35 direct edits went through Bash, and no hook refused anything.
- A model's use of hexa depends on the prompt, the installed CLAUDE.md section, the skills, the hooks and the binary. Changing any one of them can change the result, and none of them is tested against a model.
- A run of `claude -p` writes a complete record of what the model did: `--output-format stream-json --verbose` emits every tool call, every tool result (a hook refusal is a tool result whose text begins `PreToolUse:<Tool> hook error:`), and a final `result` record with `num_turns` and `total_cost_usd`.
- Scoring that record needs no model. A scorer that calls a model to judge a model is a second variable; a scorer that reads the transcript is a function, and can be tested against hand-made transcripts whose score is worked out by hand.

## Decision

1. **What is measured.** For each scenario, five numbers:
   - **outcome** — the scenario's `outcome` command, run in the scenario repo after the model stops, exits 0. Pass or fail.
   - **process** — the weighted share of the scenario's `expect_verbs` that the model's commands matched, honouring each one's order constraint.
   - **bypasses** — tool calls that went around hexa: a match of any `forbid` pattern, or a direct source edit made before any gate was recorded. A call that is both counts once.
   - **refusals** — tool results that are a hook refusal (exit 2).
   - **turns and cost** — `num_turns` and `total_cost_usd` from the transcript's `result` record.

2. **The scenario format.** One directory per scenario, `evals/hexa-usage/<name>/`, holding `scenario.toml` and `fixture/` (the starting repository files). The fields:
   - `prompt` — what a person asks, in their own words; it does not name hexa verbs.
   - `setup` — shell steps. The runner copies the fixture, runs `git init`, commits, runs `hexa init` with the binary under test and commits its files, and then runs `setup`, so a step can leave a deliberately dirty tree or add a fake remote.
   - `expect_verbs` — a list of `{ id?, pattern?, text?, weight, before_first_edit?, after? }`. `pattern` is a regex over the Bash commands the model ran; `text` is a regex over its prose, for the case where asking the person is the right move. Either matching satisfies the entry. `before_first_edit` requires the match to come before the first direct source edit (vacuously true when there is none). `after = "<id>"` requires the match to come after the match of an earlier entry.
   - `forbid` — a list of `{ label, pattern?, edit_path?, edit_content? }`. `pattern` matches Bash commands; `edit_path` and `edit_content` match Edit, MultiEdit and Write calls (all given parts must match).
   - `outcome` — a shell command that must exit 0 afterwards.
   - `outcome_fails_at_start` — when true, `--dry-run` checks that the outcome fails before the model runs, so a scenario that is already solved is caught. A vacuous scenario is a failed scenario.
   - `source` — path prefixes that count as source (default `src/`).
   - `gate_pattern` — the regex for "a gate was recorded" (default `hexa loop gate`).
   - `max_turns` and `budget_usd`.

3. **The scoring rules.** The scorer lives in `hexa-core/src/domain/usage_eval.rs` and is pure: it parses the transcript and scores it, given a regex matcher and the outcome. `hexa-exec/src/usage_eval.rs` is the use case: it parses the scenario TOML, compiles every regex (a bad one is an error, not a non-match), and calls the scorer.
   - A transcript line that is not JSON is an error naming the line. The scorer does not skip what it cannot read.
   - A refused call did not happen: it counts as a refusal and never as a match, an edit, a gate or a bypass.
   - A direct source edit is an Edit, MultiEdit, Write or NotebookEdit whose path lies under a `source` prefix, or a Bash command that writes one: `sed -i`, `perl -i`, `tee`, a `>`/`>>` redirect into it, `cp`/`mv` into it, or `python`/`node`/`ruby` code that writes (commands are split at shell separators outside quotes). `git apply` and `patch` are not seen. A command that only names a source path is not a write.
   - A gate is recorded by the first non-refused Bash call that matches `gate_pattern`. Every direct source edit before it is a bypass.
   - `process` is matched weight over total weight; with no `expect_verbs` it is 1.

4. **How runs are stored.** `hexa eval run` writes one file per run, `.hexa/evals/<timestamp>-<model>.json`, with one row per scenario: the five numbers, the result record's stop reason, and the path of the saved transcript (`.hexa/evals/<timestamp>-<model>/<scenario>.jsonl`). `hexa eval report` prints a table of the newest file and the change from the one before it. A baseline worth keeping is committed under `evals/hexa-usage/baselines/`.

5. **Real-model runs are opt-in, and they cost money.** Nothing runs a model unless a person runs `hexa eval run` without `--dry-run` or `--score`. No test does. A real run:
   - runs `claude -p "<prompt>" --output-format stream-json --verbose --max-turns <N> --max-budget-usd <budget_usd>` (plus `--model`), inside the scenario repo, with only the project's settings loaded, so the hooks and skills `hexa init` installed are the ones in force, and with the binary under test first on `PATH`;
   - is checked against hexa's daily spend budget (`inference.budget_usd_per_day`) before each scenario, counting what this run has already spent; it stops when the budget is reached;
   - runs with permission prompts bypassed, because nobody is there to answer them. The scenario repo is a temporary directory, but the model can reach the rest of the machine; run it on a machine where that is acceptable.

6. **The eval does not touch the repository it runs from**, except `.hexa/evals/`. Every scenario repo is a fresh temporary directory.

## Consequences

- A change to the hooks, the CLAUDE.md template, a skill or a verb can be measured before and after, on the same scenarios.
- Scores are only as good as the scenarios. A pattern that is too loose credits the wrong command, and one that is too tight misses the right one. The fixture tests pin the scorer, not the scenarios.
- One run per scenario is a sample of one. A difference of one scenario between two runs is noise until it repeats.
- A real run spends real money: about $1 per scenario at the starter caps.
- `evals/` holds Rust fixture projects; `hexa analyze` excludes the folder, so they do not count towards hexa's grade.

## Implementation

**Gate:** `cargo test -p hexa-exec --test usage_eval_scores && cargo test -p hexa-cli --test eval_verb`

- `hexa-exec/tests/usage_eval_scores.rs` — the scorer against the hand-made transcripts in `evals/hexa-usage/_fixtures/`: a good run, a run that bypasses with `sed -i`, a run that records the gate after editing, and a run whose outcome fails. Each expected score is computed by hand in the test.
- `hexa-cli/tests/eval_verb.rs` — `hexa eval run --dry-run` builds a scenario repo and spends nothing; `--score` on each fixture prints the same score; the run-from repository is unchanged.

## References

- ADR-2611160730: compliance by refusal, not by interface
- `evals/hexa-usage/` — the scenarios and the committed baselines
