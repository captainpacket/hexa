# hexa

<!-- hexa:agents-md:start -->
This section is written by hexa and kept current by `hexa refresh`. Every agent reads it: Codex, Cursor and Gemini read AGENTS.md, and Claude Code imports it from CLAUDE.md. Write project notes outside the markers; hexa never changes them.

### Start here

1. `hexa status`: the loop (ADR, gate, stage, checklist), what is in flight, and the next step.
2. `hexa finish`: every open item, each with the command that closes it.
3. `hexa go`: the next step alone.
4. `hexa doctor`: whether this project can orient you (hooks, this file, the graph, skills, module READMEs).

### Where builds run

- **The full gate:** not declared. Declare the command that proves the whole project as `gate.full` in `.hexa/project.json`.
- **Tests run on this machine.** A project that builds elsewhere names its runner as `test.remote` in `.hexa/project.json` (a command template with `{commit}` and `{cmd}`).

### Find your place

- Before editing a file: `hexa graph context <path>`, what it defines, what it uses, who consumes it.
- Before deleting or moving code: `hexa graph consumers <path>` across the whole workspace.
- Both read `.hexa/graph/graph.json`. `hexa graph build` makes it; a stale graph answers about old code.
- Each module's `README.md` says what it owns, what it may import, where its tests are, and which ADRs decided it.
- Lessons: `hexa memory search <topic>` before you start, `hexa memory store lesson:<topic> "<text>"` when you learn one.

### Several agents

- A lead splits the work and integrates it; builders each take files of their own, in their own worktree.
  Claude Code's definitions are `.claude/agents/hexa-lead.md` and `.claude/agents/hexa-builder.md`; the rules
  are the same for any agent.
- Split work by file. One writer per branch. Generated files change only through the tool that generates them.
- Run the formatter (`cargo fmt --all` in Rust) before every push. Wait on your own remote run; never end a turn
  while it is running. Report counts only from the remote run on the pushed head.
- Never restart a CI runner that has a job in flight.
- Integrate, never trust: the lead reruns every gate itself. The `hexa-parallel-agents` skill has the whole procedure.

## hexa — how to work in this project

hexa is one binary. There is no daemon, no database, and no dashboard. Nothing
needs to be started. Every verb below runs in this process and exits.

`hexa --help` lists the verbs. `hexa status` says where the work stands: the
loop (ADR, gate, stage, checklist), what is in flight in this repo, and the
next step (`hexa go` prints that step alone). `hexa hey <intent>` routes
natural language to a verb.

### Route work through hexa where hexa has a verb for it

| Don't do this by hand | Use this | Before you run it |
|---|---|---|
| edit, then hope it works | `hexa do run "<task>" --file <f> --evidence "<cmd>"` — a model makes the edit | Clean tree: it refuses while other tracked files in the package are modified. Name extra files with `--also`. Commits on this branch. Add `--uncounted` when `<cmd>` runs no tests |
| wrote the patch yourself, then commit it by hand | `hexa do gate --files src/parser.rs -e 'cargo test --test parser' -m 'parser: accept empty input'` — you already made the edit | Each named file must have a change; only those files are committed, and only if the evidence reports passing tests. A red or vacuous gate commits nothing and leaves the patch. Add `--uncounted` when the evidence runs no tests |
| build a whole component by hand | `hexa build '<challenge>' --target <dir> --gate '<cmd>'` | Clean tree, and no one else committing here: it commits every file that changes while it runs |
| eyeball a diff for bugs | `hexa harden <path> --gate '<cmd>'` | Clean tree, and no other session in this checkout: it resets and reverts as it fixes. Use a worktree |
| assert "the architecture is fine" | `hexa analyze .` | None; it writes nothing |
| delete code you *think* is dead | `hexa graph consumers <path>` | `hexa graph build` first: it reads the saved `.hexa/graph/graph.json` |
| read a file before editing it blind | `hexa graph context <path>` | `hexa graph build` first, as above |
| a lesson you will forget | `hexa memory store lesson:<topic> "<text>"` | None |
| claim something about the repo | `hexa verify "<claim>"` | None; it reads only |
| guess which decisions are done, or what another agent is on | `hexa adr board` (progress from evidence) and `hexa agent list` (who holds which paths) | None |
| edit paths another agent may be editing | `hexa agent register --task <name> --lease <path>`, and `hexa agent report` when done | Before the first edit |
| wonder why a native (C/C++) dependency rebuilds per command | `hexa dev build-keys` | None |

`do run` or `do gate`: use `do run` when a model should make the edit, and
`do gate` when you already made it. Both commit only the files they name, and
only on a gate that passed.

**Committing source is `hexa do gate`. `git commit` is for docs.** Commit your
own edit to project source with
`hexa do gate --files src/domain/paging.rs -e 'cargo test' -m 'paging: an even split adds no empty page'`,
not `git add` and `git commit`. The hooks refuse a `git commit` that would
commit project source, and print the `do gate` command that does it, filled in
with the files and the gate. A `git commit` of docs, ADRs, specs or markdown
only, or one that finishes a merge, is never refused. Tests written before the
code are committed while they fail, with the same command and `--expect-red`.
It commits only a red of failed test assertions, never a compile error or zero
tests, and prints the `hexa spec red` to run next. Under `test.remote` both
kinds run on the remote runner, for a pushed candidate commit, never here.
`--dry-run` shows what would be pushed and run. The last line of stdout is one
JSON object: `ok`, `commit`, `remote_head`, `tests`, and on failure `code`,
`hint`, `field`, `retryable`.

**A new session picks up where the last one stopped.** When this session has
recorded nothing, `hexa status`, `hexa loop` and the session-start banner show
the previous session's ADR, gate, stage and checklist, labelled as the
previous session's. `hexa loop resume` carries that work into this session, so
the hooks accept edits under its gate.

What each committing verb commits, on which branch, what it resets, and how to
get your work back is in the `/hexa-git-effects` skill. Read it before running
one in a tree that holds work of yours or someone else's.

### Several agents, one repository

Before you start: `hexa adr board` says which decisions are built, how far, and
who is on them; `hexa agent list` says which paths are leased. Do not start an
ADR another agent holds, and do not edit a leased path. Register your task and
its paths first (`hexa agent register`); `hexa agent report` closes it with the
commit. When your ADR's evidence is in, add a `**Built:**` line to it so the
board can see it. Each agent works in its own worktree: `hexa dev worktree`.

### Closing out work

Four moments, each with a verb. Doing them later is the same as not doing them.

- **Before you start:** `hexa agent register --task <name> --lease <path>`,
  before the first edit. A claim made after the work held nothing.
- **When you open the pull request:** it carries the ADR's `**Built:**` line
  (phases, date, commit, tests). Credit is part of the change.
- **When work merges** (yours or one you merge): `hexa adr board`. An ADR whose
  work merged and reads 0 built is yours to fix now, from the code.
- **Before you report, stop or ask what is next:** every open item is written
  down first, and it is not in your message. Write it with
  `hexa plan draft "<what is left>"`, or as an ADR for a decision. Then
  `hexa agent report`. Do not end with a menu: take the next item.

If the verb you want is missing, **build the verb**. That is how the tool grows.

If hexa's own path does not work and you have to route around it, hexa may
print a guide: follow it. Where it prints none, record the route you took —
`hexa gap add <id> --what "<…>" --around "<step>"… --fix "<path> — <change>"` —
so the gap is counted and `hexa gap pr <id>` can turn it into a pull request.

### Autonomous operation (HARD RULES)

1. **Trace consumers before you delete.** `hexa graph build`, then
   `hexa graph consumers <path>` over the whole workspace (it reads the saved
   graph, so a stale one answers about old code), then grep for the two things a graph cannot see: a
   re-export at a package root, and a conditionally-compiled import. Both have
   caused a broken build after a trace came back clean.
2. **Every change that deletes or restructures ends with a build.** A task
   marked done on a broken build is worse than no task.
3. **Never end with a menu of options.** Ship the highest-value item now. Say
   what shipped and what is left. Asking per item stalls the session.
4. **Record work; do not defer it.** Outstanding work goes in a workplan
   (`hexa plan draft <prompt>`) or an ADR, now. Never "next session".
5. **No stub tasks.** A task whose body is `echo TODO` is audit theater. Real
   work goes in a workplan; not-yet-actionable work goes in an ADR.
6. **Seek improvements proactively.** Drift or a gap → ADR → workplan.
7. **A grade below the floor is a deduction to clear, not a status to relay.**
   `hexa analyze .` names every item and its fix. Clear them, re-run, and
   report the new grade. The grade is a property of the tree, not of your
   diff: an item that predates your change is still yours to clear.
8. **Prefer `hexa hey <intent>`** when the task maps cleanly to natural language.

## Development pipeline (gate-first)

Pick the path by what the change is. The `/hexa-workflow` skill has every
step.

- **A small change** — a fix or one behaviour, no new port, adapter or
  dependency:
  1. Record the gate before the code: `hexa loop gate '<command>'`.
  2. Make the edit yourself, or have a model make it with `hexa do run`.
  3. Commit through the gate: `hexa do gate` for your own edit (`do run`
     commits its own), never `git commit`. Nothing is committed on a red
     gate.
- **An ADR-backed feature** — it adds a port, an adapter or a dependency, or
  an ADR decides it:
  1. An ADR in `docs/adrs/` (`hexa loop adr ADR-<id>`).
  2. `hexa spec draft ADR-<id>`: scenarios from the decision, never the code.
  3. **Stop. A person runs `hexa spec accept ADR-<id>`. An agent never does.**
  4. `hexa spec tests ADR-<id> --crate <dir>`: sealed tests that fail now.
  5. `hexa loop gate "hexa spec verify ADR-<id>"`, build to it, then
     `hexa harden` and `hexa analyze .`.
- **Outward-facing verbs** — `hexa pr open`, `hexa issue file`,
  `hexa issue sync`, `hexa gap pr`. By default they print what they would
  send. Pass `--send` only when the person has said to, in this conversation.

List the steps with `hexa loop task add "<step>"` and check each off with
`hexa loop task done N`; `hexa loop` shows where the work stands. The hooks
stop a feature-sized edit that has no gate recorded.

A project declares its full gate, the command that proves the whole project,
as `gate.full` in `.hexa/project.json`: `"gate": {"full": "cargo test"}`. With
it declared, a narrower command such as
`hexa loop gate 'cargo test --test paging'` is recorded as the fast gate, for
quick checks while you work, with a warning. The full gate still decides
`hexa loop stage done` and is the evidence `hexa do gate` is given.

A gate passes on what a test runner reports, not on exit 0 alone. A gate that
runs no tests (a build, a lint, `hexa analyze . --exit-code`) is declared
`--uncounted`, on `hexa loop gate` and on `hexa do run`, or it fails.

Three rules govern it:

- **A spec that cannot be run does not exist.** It becomes a gate, or it
  becomes ADR prose. ADR prose is history, and history is allowed to be
  unexecutable, because it never claims to describe the present.
- **The gate is written before the code, and is not derived from it.** A gate
  generated from the implementation tests the implementation against itself.
- **A vacuous gate is a failed gate.** "0 tests passed" is not a pass. Every
  new gate shape needs that guard.

## Hexagonal Architecture Rules (ENFORCED)

`hexa analyze .` checks these:

1. **domain/** imports only from **domain/**.
2. **ports/** imports from **domain/** only, for value types.
3. **usecases/** imports from **domain/** and **ports/** only.
4. **adapters/primary/** imports from **ports/** and **usecases/**: a driving
   adapter may call a use case. Never **domain/** directly.
5. **adapters/secondary/** imports from **ports/** only. A driven adapter
   calling back into **usecases/** inverts the dependency.
6. **Adapters NEVER import other adapters.**
7. The **composition root** is the only file that imports an adapter.
8. Relative imports in TypeScript use `.js` extensions (NodeNext).

Rule 4 has a consequence people miss: if an adapter needs a domain type, the
**port** re-exports it. The adapter imports the port. Otherwise every adapter
grows a second edge into the core.

## File Organization

```
src/
  domain/            # Pure business logic, zero external deps
  ports/             # Typed interfaces — the contracts between layers
  usecases/          # Application logic composing ports
  adapters/
    primary/         # Driving adapters (CLI, HTTP, browser input)
    secondary/       # Driven adapters (DB, API, filesystem)
  composition-root   # Wires adapters to ports — the single DI point
```

## Lessons that a rule cannot catch

`.hexa/ADR-rules.toml` enforces the lessons a pattern can find. `hexa analyze .`
runs them, and each one cites the incident that produced it. These are the rest
— real failures with no pattern to match. Read them before you trust a green
result.

- **A gate that degrades silently is worse than no gate.** A boundary check
  that falls back to a compile check when its analyzer is unreachable turns "no
  boundary violations" into "it compiles" — a different claim — and still
  prints a result. Fail loudly. *(Not enforceable: no pattern separates a gate
  falling back from an ordinary default for an optional flag. A rule for this
  was written, fired only on correct code, and was removed —
  `.hexa/ADR-rules.toml` records why.)*
- **Tests can mirror bugs.** The same model writes the code and the test, so the
  test encodes the same misunderstanding. Use property tests and an independent
  oracle. Worse: a test that redefines its subject asserts against a copy of the
  design, not against shipped code. *(Partly enforced:
  `test-must-not-redefine-its-subject`.)*
- **"It compiles" is not "it works".** Add a check that a user can actually run
  the thing.
- **The write shape and the read shape must agree.** A record persisted with a
  string id and read back into a numeric field fails to deserialize on every
  row. If the read discards errors, the feature reports empty and looks healthy.
  Never discard a deserialize error you did not expect.
- **A cache in front of a file that only a short-lived process reads is not a
  cache.** It is a second source of truth that can disagree with the first.
- **Documentation about a deleted feature never fails.** It sits there being
  confidently wrong. When you delete a feature, grep the docs in the same
  change.
- **Parallelize by file boundary; serialize by file overlap.** Two agents
  editing one file produce conflicting diffs.
- **Sign conventions matter.** For physics and maths, write the coordinate
  system down.
<!-- hexa:agents-md:end -->

## Project notes

Hand-written: hexa never changes anything outside the markers above.

- **hexa's own operator manual is [`CLAUDE.md`](CLAUDE.md), and it is binding for every agent, not only Claude Code.**
  It names the crates, the tiers, the hard rules and the gate-first pipeline as this repository applies them. Where
  it is more specific than the guide above (the file layout is a Cargo workspace of seven crates, not `src/`), it wins.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) is the map; each crate's `README.md` says what it owns and may import.
- `founding-goals.md` is the one file no agent may touch.
- Never write a model or provider name outside `hexa-infer`.
