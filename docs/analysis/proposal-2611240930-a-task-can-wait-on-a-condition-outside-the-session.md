> **Proposal, not yet an ADR** (2026-11-24). Kept here until the owner accepts it; see the Gate line.

# ADR-2611240930: A task can wait on a condition outside the session

**Status:** Proposed
**Date:** 2026-11-24
**Drivers:** datum, 2026-11-24 (gary: "Yes, all four", the fourth being the stop hook). A plan's last steps are often not this session's to finish: "Land on main" waits on a pull request's review, a CI run on the forge, or another thread's merge. The stop hook sends unfinished work back (`hexa finish`, up to three times in a row), and every turn of every session in the repository is spent being told about the same step that nothing here can move. Agents learn to clear or remove a step to get the hook to stop, which loses the record of what is left.
**Supersedes:** none. Would amend ADR-2611161800 §2 and §3 (the close-out steps, and what `stage done` requires) and the stop hook's rule.

## Context

A loop task is `todo`, `doing` or `done` (`hexa-cli/src/commands/loop_cmd.rs`). The stop hook and `hexa finish` count every task that is not `done` as open work for this session. Nothing can say "this step is open, it is not this session's to move, and here is how anyone can tell when it is". That is different from done. The work is not finished, and `stage done` must still refuse. And it is different from todo, because nothing in this session can move it.

This changes what "done" and "open" mean for the hooks, so it is for the owner to accept. It is not built.

## Decision (proposed)

1. **A task may be `waiting`, with a reason and a condition that can be checked.**
   `hexa loop task wait N --on '<condition>' --reason '<why>'`. A condition is one of:
   - `pr <forge>/<owner>/<repo>#<n> merged` (or `closed`), read through the forge port that `hexa pr` uses;
   - `ci <forge>/<owner>/<repo>@<ref> green`, read the same way;
   - `cmd '<shell command>'`: the condition holds when the command exits 0. It is run with a timeout, at most once a minute per task. It is never run under `test.remote` if it builds: a condition is a read, not a gate.
   
   A condition hexa cannot parse is refused when it is set, naming the forms.

2. **The stop hook does not send back a session whose open tasks are all `waiting`.** It prints one line per waiting task (its reason and condition) and lets the turn end. A `todo` or `doing` task still sends the work back, as today.

3. **A waiting task is checked, not trusted.** `hexa status`, `hexa finish` and the session-start banner evaluate each condition. One that now holds turns its task back to `todo`, with a line saying so, so the next turn takes it. One that cannot be evaluated (the forge unreachable, the command missing) is shown as `waiting (unchecked: <why>)`, and the stop hook treats it as waiting for at most 24 hours from when it was set. After that it is `todo` again: a condition nobody can check is not a reason to stop forever.

4. **Waiting is not done.** `stage done` still refuses while any task is `waiting`, naming it. `hexa loop task done N` on a waiting task is allowed, and records that it was waiting and on what. `hexa finish` lists waiting tasks in their own section, each with its condition and the command that would check it now.

5. **Recorded, not hidden.** `.hexa/loop.json` stores, for each waiting task, `status: "waiting"`, `on` (the condition as written), `reason`, `since` and `checked` (the time and result of the last check). `hexa loop` shows them.

## Consequences

- A plan whose remaining steps all wait on a review or a merge no longer burns every turn. The record of what is left stays.
- The forge read becomes a dependency of the stop hook, which today reads only files. Decision 3's timeout and its 24-hour limit bound that.
- An agent can still abuse `waiting` to silence the hook. Decision 3 (a condition that holds turns the task back), decision 4 (`stage done` refuses) and the 24-hour limit on an unchecked condition are the guards. Whether those guards are enough is the owner's call at acceptance.

**Gate:** none yet. This is a proposal, not an ADR: it moves into `docs/adrs/` as ADR-2611240930 when the owner accepts it, with its spec as its gate (`hexa spec verify`). It is kept out of `docs/adrs/` until then because every ADR there must record a gate (the without-a-gate ledger may not rise) and `adr.unbuilt` holds at most two (ADR-2610271110).
