---
name: hexa-maintain
description: Keep a hexa install and a hexa project current — the binary, the managed sections of AGENTS.md and CLAUDE.md, the embedded skills, agent roles and assets, the module READMEs, the docs, the API contract, and the project's own checks. Use when the user asks "is hexa up to date", "update hexa", "refresh CLAUDE.md", "sync the skills", "check the docs", "check the API contract", or "run the checks".
---

# hexa maintain — the install and the project, current

## The binary

```bash
hexa --version          # the version and the commit it was built from
hexa self-update --check
hexa self-update        # the newest published release
hexa doctor             # every check, including whether this binary is behind
```

`hexa self-update` installs releases. A binary built from a checkout is
current only with that checkout: a session opens with a warning when it is
behind, and the warning carries the rebuild command.

## What hexa manages inside a project

```bash
hexa doctor             # can this project orient an agent? each failing piece with the command that fixes it
hexa refresh            # the hexa-managed sections of AGENTS.md and CLAUDE.md, in place; never your own text
hexa skill list         # skills embedded in the binary, and the project's own
hexa skill sync         # write the embedded skills into .claude/skills/ and the lead and builder roles into .claude/agents/; names any whose copy differs
hexa skill sync --update   # replace those (only skills the binary carries; your own are untouched)
hexa skill show <name>
hexa assets list        # everything embedded: scaffold templates, provider presets, skills, schemas
hexa assets sync        # write them into .claude/
```

`hexa skill sync` never overwrites, so a skill that changed in the binary does not
reach a project that already has it on its own: it names the ones that differ, and
`--update` replaces them (a skill you edited by hand is replaced too). After
`hexa self-update`, run `hexa refresh` and `hexa skill sync --update`.

AGENTS.md is the guide every agent reads (Codex, Cursor and Gemini read it;
CLAUDE.md imports it with the line `@AGENTS.md` and adds only what Claude Code
uses). `hexa refresh` rewrites only the text between each file's
`<!-- hexa:…:start -->` and `<!-- hexa:…:end -->` markers. It creates AGENTS.md
when it is missing, appends the section to one that has no markers, and refuses
a file whose markers are out of order. `hexa doctor` and `hexa finish` say when
either is out of date.

## The docs

```bash
hexa docs check         # freshness and terminology across docs/, and a module with no README
hexa docs readmes       # write a README (owns / may import / tests / ADRs) into each module that lacks one
hexa docs glossary      # the terms `check` enforces
hexa adr doctor         # every ADR id cited anywhere resolves to a file
```

## The API contract

For a project whose driving ports carry `@hexa:api` tags:

```bash
hexa api list           # one line per operation
hexa api spec           # write the OpenAPI document the tags declare
hexa api check          # exit 1 when the committed document has drifted
hexa api adapter        # generate the primary HTTP adapter for a tagged port
hexa api test --base-url http://127.0.0.1:8080   # every operation against a running test instance
```

`api test` creates and deletes data. Point it at a test instance only.

## The project's own checks

```bash
hexa dev ci             # every hexa enforcement gate
hexa dev validate       # build → test → analyze → validate
hexa dev worktree list  # every active worktree
hexa dev worktree cleanup           # dry run: the merged worktrees it would remove
hexa dev worktree cleanup --force   # remove them; a worktree with uncommitted changes is kept
hexa status             # the loop, what is in flight in this repo, the next step
```

`cleanup --stale` adds unmerged feature worktrees idle for a day. Their
branches are kept, and git still refuses any worktree with uncommitted work.

## Loose ends in transcripts

```bash
hexa insight punch-list   # numbered gaps and follow-ups in recent transcripts that nothing routed anywhere
```
