# ADR-2611160730: Compliance by refusal, not by interface

**Status:** Proposed
**Date:** 2026-11-16
**Drivers:** An adversarial audit of how hexa is really used in the harness project (four independent read-only reviews, 2026-11-16). Agents route about 8% of edits through hexa and none of the code edits; the person had to say "use hexa for everything", and the agent stored that as a lesson and then kept editing by hand. The question raised was whether to drive hexa over MCP.
**Supersedes:** none. Companion to ADR-2611152035 (hexa learns from its own record).

## Context

What the audit found, with evidence in the hexa source at `fd463ba`:

1. **The hooks guard the tools agents don't use.** `pre_edit` blocks with exit 2 only for Edit/Write, only when the prompt was sized T2/T3, and only if *some* gate string exists in `loop.json` (`hexa-cli/src/commands/hook/mod.rs:767-788`). `pre_bash` (`:879-903`) only warns. In the one surviving working transcript, 33 of 35 direct edits went through Bash (`python3` patch scripts, `sed -i`, `cat >`). No hook blocked anything all session.
2. **The verbs don't fit how an agent works, and two are unsafe.**
   - `do run` gives the edit to a second model. There is no verb for "gate this diff I already wrote".
   - `do run` also commits every tracked dirty file in the target's *package*. In a repo with a root `Cargo.toml` that is the whole repo (`hexa-exec/src/direct_exec.rs:1342-1355, 1583-1606`). This happened on 2026-11-16 (`218c945`, undone by hand).
   - `harden` soft-resets HEAD when HEAD moved during a fix, whoever moved it (`adversarial.rs:1276-1278`). It reverts or commits any file that changed during the agent's run (`:891`). Fast-gate batching uses `reset --keep` plus cherry-pick, which drops commits made in the meantime (`:1493, 1562, 1716`). All of this was reproduced in a throwaway repo by replaying the same git commands.
   - `build` sweeps edits made at the same time into its commit and skips the seal check (`:1884-1890`).
3. **False greens.**
   - Harden's full gate and build's gate accept any exit 0, including a run with 0 tests (`run_gate` `:116`, `:1420`, `:1880`). Only the fast gate checks for a vacuous pass.
   - Harden reports "fixed" without running the new test against the old code. The prompt asks for such a test (`:1265`) but nothing checks it was red first.
4. **Lessons are kept but not used.**
   - `do run` reads up to 6 memory entries (`direct_exec.rs:913`).
   - `harden` and `build` read none (`adversarial.rs:1127, 1192, 1263, 1783, 1812, 1869`). That is why harden fixed seven instances of one async-staleness bug one at a time after `lesson:async-staleness` was stored.
   - Gaps go to a global `~/.hexa/gaps.jsonl` that no prompt reads.
   - `adr accept` throws away the rationale it asks for.
5. **The guidance is wrong in places.**
   - The skills promise `do run` is "one file" (`hexa-workflow/SKILL.md:83`, `hexa-analyze-arch/SKILL.md:33`).
   - The analyze-arch fix recipe fails every time because `--uncounted` is missing (`:30`).
   - `npx hexa summarize` does not exist, and the hexa-summarize skill is dead.
   - `adr schema` claims a `POST /api/adr/reserve` that does not exist.
   - `hey --queue` does nothing.
   - CLAUDE.md and hexa-workflow describe different pipelines: no `spec` step, no person-accepts stop.
   - `analyze` is marked hidden while everything promotes it.
   - `graph consumers` reads a snapshot that may be stale or missing.
   - The `route` hook drafts workplans in the background, even for read-only prompts, which contradicts "nothing runs on its own".
6. **MCP was tried and removed.** The ADR-019 CLI/MCP parity suite went with the daemon (ADR-2608241500; `commands/test.rs:5, 522`). The reasons were not recorded.

## Decision

**hexa gets compliance by refusing unsafe or ungated work, not by changing its interface. No MCP server now.**

MCP would add typed parameters, discoverability and structured results. On its own it forces nothing while Edit and Bash stay open. Closing them needs the same hook and permission work that makes the CLI the only path. It also brings back a second surface to keep in parity, MCP timeouts against 7–20 minute verbs, version skew from a long-lived server, and a schema token cost in every session.

In order of priority:

1. **Make `harden` and `build` safe to run beside a person.**
   - Refuse to start unless the tree is clean and HEAD is the branch tip, and take a per-repo lock.
   - After the agent returns, fold back only commits authored by hexa; stop and report otherwise.
   - Never revert a file that changed outside the agent's own writes. Save it to `.hexa/harden-rescue/` first.
   - Before every `reset_to`, check that HEAD is still hexa's last commit.
   - The end state is ADR-2610221834: one worktree per fix.
2. **`do run` commits only `--file` and `--also`.**
   - Remove the start-snapshot package sweep. A prefix of `""` must never mean "everything".
   - Refuse when a named file was already dirty, unless `--include-dirty` is passed.
   - Stop writing `last_run` over loop state the run does not own.
3. **A verb for the agent's own patch:** `hexa do gate --files <f>… --evidence <cmd>`. It runs the evidence on the edit already in the tree, checks for a vacuous result, and commits only the named files. This is the path an agent will actually use.
4. **The hooks see what agents really use.**
   - `pre_bash` exits 2 on write-shaped commands aimed at project source when no gate is recorded for the current task: `sed -i`, `perl -pi`, `>`/`>>`/`tee` into tracked paths, and `python`/`node` running a script that writes to the repo.
   - The `pre_edit` gate check runs for every source edit, not only T2/T3 prompts, and is tied to the current loop task, not to "some gate exists".
   - `hexa init --strict` (opt-in) writes `deny: Edit(src/**), Write(src/**)` and allows `Bash(hexa do *)`.
5. **No false greens.**
   - Every gate result goes through `evidence_verdict` and fails on `Vacuous`.
   - Harden calls a fix "fixed" only after the new test was shown red without the fix; otherwise it reports "untested".
6. **Lessons reach every prompt.**
   - `harden` and `build` call the same lesson gathering as `do run`, plus any `ADR-rules.toml` rules matching the target. This is ADR-2611152035 §4.
   - Gaps get a project scope and are read the same way.
   - `adr accept` records its rationale as a dated line in the ADR, so a hand edit can be detected.
7. **Guidance that is procedure, and that is checked.**
   - One `hexa-git-effects` reference: for each verb, what it commits, resets and requires, and how to recover.
   - CLAUDE.md as a decision tree: a small change is gate only; an ADR-backed change follows the spec path; outward-facing verbs use `--send` only with consent.
   - Fix or delete the broken skill lines above.
   - A CI check runs `--help` on every `hexa …` line in `assets/skills/**`.

MCP is reconsidered after two weeks of `observe-pre` data with items 1–4 in place, and only if `hexa`-through-Bash calls still fail in ways a schema would fix, such as quoting errors or misread output. If it is built, it is a stdio `hexa mcp` that starts one fresh hexa process per call and keeps no state between calls. It would expose five tools: gate, do, job_start, job_status and query, with long verbs running as detached jobs under `.hexa/jobs/<id>/`. A parity test would run each tool against its CLI equivalent.

## Consequences

- Agents lose the Bash route around the gate. Some legitimate scratch writes will be blocked until the write-shape matcher is tuned. The matcher fails closed only for tracked project paths.
- `harden` can no longer run while a person commits in the same tree. That is the point: today the result is silent loss.
- `do run` commits get smaller, and a user who relied on the sweep must pass `--also`.
- Stricter gate checks will turn some current "passes" red. Those were false greens.
- Keeps the no-daemon principle (ARCHITECTURE.md:10).

## Implementation

**Gate:** `cargo test -p hexa-exec --test do_commit_scope --test do_gate --test harden_concurrent --test harden_false_greens --test build_refuses_unsafe_work && cargo test -p hexa-cli --test skills_name_real_commands --test hook_pre_bash`.

Gate first. Each item ships with a failing test written before the change:

1. `hexa-exec/tests/harden_concurrent.rs`: a commit made during a fix survives; a file edited outside the agent is not reverted.
2. `hexa-exec/tests/do_commit_scope.rs`: extend it so a dirty unrelated tracked file in the same package (root `Cargo.toml`) is *not* committed.
3. `hexa-cli/tests/hook_pre_bash.rs`: `sed -i s/a/b/ src/x.rs` with no gate exits 2; `cargo test` exits 0.
4. `run_gate` on `true` and on a "0 tests" runner output is `Vacuous` and fails.
5. A harden fix whose test passes without the fix is reported "untested".
6. The harden prompt includes a stored `lesson:*` that matches the target.
7. The skills CI check fails on `npx hexa summarize`.

## References

- ADR-2611152035: hexa learns from its own record and gives better models bigger work
- ADR-2610221834: isolate harden fixes in worktrees
- ADR-2608241500: the daemon (and the MCP server) removed
- ADR-019: CLI/MCP parity
- harness gaps `do-run-commits-whole-tree` and `harden-resets-others-commits`
