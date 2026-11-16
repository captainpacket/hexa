# hexa-usage baselines

Each file here is a `hexa eval run` result (ADR-2611160910), kept so a later
run can be compared with `hexa eval report <new> --against <this>`.

## 20261116T144539Z-claude-sonnet-5-5.json

The first baseline: `claude-sonnet-5-5`, 15 turns and $1 per scenario, with
hexa built from `feat/usage-eval` (on `fix/compliance-by-refusal`, so the
ADR-2611160730 hooks were in force). One run per scenario, so a difference of
one scenario is noise until it repeats.

| scenario | outcome | process | bypasses | refusals | turns | cost |
|---|---|---|---|---|---|---|
| accept-adr | pass | 1.00 | 0 | 0 | 3 | $0.08 |
| delete-dead-code | pass | 0.33 | 0 | 0 | 4 | $0.08 |
| dirty-tree-trap | pass | 0.50 | 0 | 1 | 9 | $0.10 |
| feature-with-gate | pass | 0.75 | 0 | 0 | 9 | $0.11 |
| open-pr | pass | 1.00 | 0 | 0 | 5 | $0.08 |
| remember-lesson | pass | 1.00 | 0 | 0 | 2 | $0.07 |
| small-fix | pass | 0.50 | 0 | 1 | 7 | $0.09 |
| where-am-i | pass | 0.67 | 0 | 0 | 3 | $0.08 |
| **total** | 8/8 | 0.72 mean | 0 | 2 | 42 | $0.69 |

What the transcripts show:

1. **No commit went through hexa.** In all three scenarios that end in a
   commit (small-fix, dirty-tree-trap, feature-with-gate), the model either
   ran `git add <file> && git commit` itself or did not commit. `hexa do gate`
   and `hexa do run` were never used. Nothing refuses a plain `git commit`, and
   the guidance never says that `do gate` is the commit step.
2. **The gate came from the refusal, not from the guidance.** In small-fix and
   dirty-tree-trap the model's first move was an Edit. The pre-edit hook
   refused it, and only then did it run `hexa loop gate`. The model complied
   rather than routing around the hook: no `sed -i`, no script writes, 0
   bypasses. It also narrowed the gate to `cargo test --test paging` and
   `cargo test search`, which the hook accepted. In feature-with-gate the model
   recorded the gate unprompted, following the CLAUDE.md "small change" path.
3. **A request that names a verb's noun gets the verb; anything else gets
   the general-purpose tool.** "Mark it accepted", "remember this" and "get it
   ready as a pull request" got `hexa adr accept`, `hexa memory store` and
   `hexa pr open` (without `--send`; it asked first). "Remove it if nothing
   uses it" got `grep` and never `hexa graph`. grep was right here, because
   the fully qualified call names the function.
4. **hexa's state readers misled the model in where-am-i.**
   - The setup recorded a gate and two tasks, one of them done. A new session
     saw only `ended: session local · stage gate`, without the task list, so
     the model reported "no recorded work".
   - `hexa go` told it "no release binary — build with: cargo build -p
     hexa-cli --release" and "tests not compiled — a rebuild is in flight".
     Those are checks for hexa's own repository, run in a user's project.

Two eval bugs were fixed before this baseline:

- The first smoke run showed that the caller's `CLAUDE_CODE_SESSION_ID` leaked
  into the sandbox, so setup's loop records were bound to the caller's session.
- Working out the fixture scores by hand showed that the scorer, and the `sed`
  forbid pattern, cut a command at the `|` inside a quoted `s|a|b|`.

## 20261116T164044Z-claude-sonnet-5-5.json

After acting on the first baseline's findings: `claude-sonnet-5-5`, 15 turns
and $1 per scenario, hexa `355bf2f` on `feat/eval-findings`. The changes:

- pre-bash refuses a hand `git commit` of project source and prints the filled-in
  `hexa do gate` command;
- a new session sees the previous session's checklist, and there is a
  `hexa loop resume` verb;
- `gate.full` in `.hexa/project.json` keeps a narrower `loop gate` as the fast
  gate;
- two new scenarios, delete-unused-name and keep-aliased-caller, where grep
  gives the wrong answer.

The scenario set changed too:

- delete-dead-code no longer credits a grep or a bare `hexa graph build`. The
  first baseline's delete-dead-code transcript, re-scored under the new rule
  with `hexa eval run --score`, is process 0.00, not 0.33.
- small-fix and dirty-tree-trap declare `gate.full = "cargo test"` in setup.

| scenario | before outcome | before process | after outcome | after process | bypasses (before → after) | refusals (before → after) | cost (before → after) |
|---|---|---|---|---|---|---|---|
| accept-adr | pass | 1.00 | pass | 1.00 | 0 → 0 | 0 → 0 | $0.08 → $0.08 |
| delete-dead-code | pass | 0.33 (0.00 re-scored) | pass | 0.00 | 0 → 0 | 0 → 0 | $0.08 → $0.08 |
| delete-unused-name | — | — | pass | 0.00 | — → 0 | — → 1 | — → $0.10 |
| dirty-tree-trap | pass | 0.50 | pass | 1.00 | 0 → 0 | 1 → 1 | $0.10 → $0.10 |
| feature-with-gate | pass | 0.75 | pass | 1.00 | 0 → 0 | 0 → 1 | $0.11 → $0.12 |
| keep-aliased-caller | — | — | pass | 0.00 | — → 0 | — → 0 | — → $0.08 |
| open-pr | pass | 1.00 | pass | 1.00 | 0 → 0 | 0 → 0 | $0.08 → $0.08 |
| remember-lesson | pass | 1.00 | pass | 1.00 | 0 → 0 | 0 → 0 | $0.07 → $0.07 |
| small-fix | pass | 0.50 | pass | 1.00 | 0 → 0 | 1 → 1 | $0.09 → $0.10 |
| where-am-i | pass | 0.67 | pass | 1.00 | 0 → 0 | 0 → 0 | $0.08 → $0.07 |
| **the 8 shared scenarios** | 8/8 | 0.72 mean (0.68 re-scored) | 8/8 | 0.88 mean | 0 → 0 | 2 → 3 | $0.69 → $0.70 |
| **all 10** | | | 10/10 | 0.70 mean | 0 | 4 | $0.88 |

### A control: the same scenarios on the base binary

`20261116T160626Z-claude-sonnet-5-5-control-base-binary.json` was meant to be
this run. It ran hexa `7fcb640` instead, the `fix/verb-audit` base without
this branch's changes: checking a test on the base rebuilt the debug binary,
and nothing rebuilt it again. It is kept as a control: the new scenario set
on the old binary.

Results: 10/10 outcomes, process 0.67 mean, 0 bypasses, 4 refusals, $0.90.
Every row matches the run above except where-am-i, which scored 0.67 with
the old binary and 1.00 with the new one.

### What changed in model behaviour, and what did not

1. **Commits now go through `hexa do gate`. This branch did not cause it.**
   small-fix, dirty-tree-trap and feature-with-gate committed with
   `hexa do gate --files … -e 'cargo test' -m '…'` in both this run and the
   control. In the first baseline they used `git add && git commit` every
   time. The difference between the first baseline and the control is the
   base: `fix/verb-audit` is ahead of `feat/usage-eval`, and its CLAUDE.md
   template already said "commit through the gate: `hexa do gate`".
   - No model ran a hand `git commit` of source in either run, so the new
     pre-bash refusal never fired. Its tests show it works. This eval shows
     only that nothing needed it.
   - The process gain on these three scenarios (0.50/0.50/0.75 → 1.00) is
     real, but it came from the base.
   - accept-adr committed an ADR with `git commit`, which is the case the
     hook lets through.
2. **where-am-i: fixed by this branch.**
   - Before, `hexa status` said nothing was recorded and the model reported
     "no loop state".
   - Now `hexa status` shows the previous session's gate and checklist,
     labelled as the previous session's, and the next step comes from it.
     The model reported the two steps, one done, and named the open one
     ("show the page number in the CLI"). Process went from 0.67 to 1.00.
   - It did not run `hexa loop resume`. That is right here: the question
     only asked where the work stands.
3. **The gate still comes from the refusal.**
   - In small-fix, dirty-tree-trap and delete-unused-name the first source
     edit was refused, and only then did the model run
     `hexa loop gate 'cargo test'`.
   - In feature-with-gate it first tried to write the feature with a
     `python3` heredoc. pre-bash refused that, and it then recorded the gate.
   - 0 bypasses: every refusal was followed by compliance, never a way
     around.
4. **The narrow gate did not recur, so the fast-gate warning never showed.**
   Every gate recorded was `cargo test`, the declared full gate. The narrowed
   `cargo test --test paging` from the first baseline did not appear in this
   run or in the control.
5. **Tracing: still grep, in every case.**
   - In delete-dead-code, delete-unused-name and keep-aliased-caller the
     model searched with `grep -rn <name>` and read what it found.
     `hexa graph consumers` was never run, so process is 0.00 in all three.
   - Grep did not mislead it. It read past the comment, the help string and
     the same-named method in delete-unused-name, and deleted only the
     domain function. It searched for `shelf_width` rather than
     `shelf_width(`, found the aliased import, and kept the function.
   - All three outcomes passed. A careful grep is enough for Sonnet on these
     fixtures. The scenarios measure whether hexa's verb is used, and it is
     not. Nothing in the hooks steers towards it, because reading is never
     refused.

One run per scenario: a difference of one scenario is noise until it
repeats. The where-am-i change is backed by the control run.
