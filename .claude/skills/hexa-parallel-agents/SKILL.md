---
name: hexa-parallel-agents
description: Run builder agents in parallel, each in its own git worktree, then integrate and verify their work. Use when the user asks to "run in parallel", "use multiple agents", "fan out", "split this up", or when work splits cleanly by file (gate tests before code, independent modules, docs beside code).
---

# Parallel builder agents

Split work by file: parallelize by file boundary, serialize by file overlap. Two agents editing one file produce conflicting diffs. If the work shares a file (one schema, one rule module, one index), one agent builds it, or the lead does.

The roles are `hexa-lead` (`.claude/agents/hexa-lead.md`), which splits and integrates, and `hexa-builder` (`.claude/agents/hexa-builder.md`), which takes one file-scoped piece. Run builders with the harness's Agent tool and `isolation: "worktree"`. hexa guards it (`hexa hook pre-agent`), and `hexa dev worktree` lists the worktrees.

`hexa plan execute` is a sequential, in-process runner in one checkout. It is fine for one small task, but wrong for splitting work across agents. `hexa build` and `hexa harden` are not parallel editors either.

## Before you start

1. The work has a recorded gate (`hexa loop gate '<cmd>'`). The hooks stop an edit without one.
2. **Split work by file.** Write down which agent owns which files. If two agents need the same file, do not parallelize that part.
3. **One writer per branch.** Each builder writes only on its own worktree branch. Nobody else commits there, the lead included, until the builder has reported.
4. Check what the agents will see. A worktree starts from the remote's base branch, not from your working tree. Uncommitted files (a new ADR, a spec) are not there: give their absolute paths, or commit and push them first.
5. **Run at most two compile-heavy agents at once.** More than that saturates the build hosts, and timing-sensitive tests start failing for reasons that are not the change.
6. Read AGENTS.md's "Where builds run". If the project names a remote runner (`test.remote`), every builder tests there. Nobody compiles on the lead's machine.

## The brief

An agent knows only what the brief says. Include:

- the goal, and the path of the spec (the ADR section is the spec), as an absolute path when it is uncommitted;
- the exact files it owns, and an explicit off-limits list: sealed spec tests, gate scripts, CI configuration, other agents' files;
- the definition of done and the command that proves it, with the test counts it must report;
- a build directory of its own, never shared with another checkout (a shared build directory can serve one checkout's artifacts to another);
- "commit on your worktree branch and push it; test on the runner AGENTS.md names; no merge, no PR";
- "run the formatter before every push";
- "wait on your own remote run; never end your turn while it is running";
- "report counts only from the remote run on the pushed head";
- "generated files change only through the tool that generates them";
- "never restart a CI runner with a job in flight";
- "stop and report if the spec or a sealed test looks wrong" (agents have found real spec defects this way);
- what to report: commit hashes, worktree path, the red/green table, every judgment call the spec did not decide, and what was not verified.

Start builders in the background and wait for each one's notification. Do not end your own turn while a builder you started is still running: the work is not integrated, so it is not done.

## While they run

Use `hexa dev worktree list` to see the worktrees. Do not edit the files an agent owns.

## Integrate, never trust

An agent's "green" is a claim. For each finished agent, in this order:

1. Cherry-pick its commits (`git cherry-pick -x`) onto the integration branch.
2. Run the formatter, push, and rerun **every** gate yourself on the runner AGENTS.md names, with the test counts shown. Zero tests run is a failure. Quote counts only from the run on the pushed head.
3. Check that a "red" test is red for the right reason by reading the first failure message. A compile error is not a red test.
4. Check that the sealed paths are unchanged since the spec was accepted. `hexa spec verify <ADR>` must pass.
5. Spot-check at least one claim by running it. A report once said two error codes were documented when one was missing.
6. Agents may skip `hexa do gate` even when told to use it. Rerunning the gate yourself is the check; say so in the report.
7. Put the agents' judgment calls, where the ADR was silent, into an ADR addendum.

## Clean up

Remove finished worktrees before `hexa analyze .`, because it scans worktrees under the project and caps the grade:

```
git worktree remove --force <worktree path>
hexa dev worktree cleanup
```

## Limits

- A worktree-isolated subagent cannot run git against another repository. For work in another repository, make that repository's worktree yourself from the main session, give the agent file tools only, and commit its files yourself.
- A subagent cannot start subagents. Only the main thread delegates.
- The agent's report is not shown to the person. Relay what matters, including what is not done.
