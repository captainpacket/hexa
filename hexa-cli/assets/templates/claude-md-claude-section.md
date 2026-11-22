@AGENTS.md

## hexa — what only Claude Code uses

AGENTS.md, imported above, is this project's guide for every agent: where to start, where builds run, the
verbs, the gate-first pipeline and the layer rules. It is not repeated here. This section is the part only
Claude Code reads. `hexa refresh` keeps both current.

**The hooks** (`.claude/settings.json`, each one runs `hexa hook <event>`):

- When a session starts, it prints where the work stands and the previous session's loop.
- Before an edit, it refuses a feature-sized edit with no gate recorded (`hexa loop gate '<cmd>'`).
- Before a shell command, it refuses a `git commit` of project source, and prints the `hexa do gate` command that does it.
- When the session stops, it sends unfinished work back (`hexa finish`), at most three times in a row.

A refusal says why and what to run. Follow it rather than working around the hook.

**The skills** (`.claude/skills/`, installed by `hexa init`, kept current by `hexa skill sync`):

- `/hexa-workflow` is the gate-first loop, verb by verb.
- `/hexa-git-effects` says what each committing verb does to git. Read it before running one in a tree that holds work.
- `/hexa-parallel-agents` is how to split work across builders and integrate it.

`hexa skill list` lists them all.

**The agents** (`.claude/agents/`):

- `hexa-lead` runs a piece of work end to end. Use it as the main thread: `claude --agent hexa-lead`.
- `hexa-builder` takes one file-scoped piece of work to a recorded gate.

Spawn a builder with the Agent tool and `isolation: "worktree"`, so that each one writes on its own branch. A
subagent cannot start subagents: only the main thread delegates.

A project's own copy of a skill or an agent wins. `hexa skill sync` never overwrites a copy that differs. It
names it, and only `--update` replaces it.
