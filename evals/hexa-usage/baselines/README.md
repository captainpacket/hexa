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
