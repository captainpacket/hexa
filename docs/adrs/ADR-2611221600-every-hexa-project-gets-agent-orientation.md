# ADR-2611221600: Every hexa project gets agent orientation

**Status:** Accepted
> 2026-11-22 Accepted: the owner asked for it in so many words (gary, 2026-11-22: "how do we make sure that the agents know how to use hexa effectively for every project? … get on the hexa [work] and update that too"), under the standing instruction recorded in ADR-2611191120 (always accept the ADRs and specs, and land the work); recorded by the implementing agent on his behalf
**Date:** 2026-11-22
**Drivers:** The owner (above). The workplan draft `draft-2611221546-every-hexa-project-gets-agent-0669d0` (recorded in the main checkout), written from what datum's agents got wrong on 2026-11-22.
**Supersedes:** none. Amends ADR-2611160730 §4 (what `hexa init` installs) and ADR-2611161800 §1 (what `hexa finish` lists).

## Context

`hexa init` writes `.hexa/`, a CLAUDE.md with one hexa-managed section, the hooks, and the generic skills. On datum that was not enough for its agents to use hexa well:

- **Only Claude Code was told anything.** Codex, Cursor and Gemini read `AGENTS.md`, and hexa never writes one. The guide lives in CLAUDE.md, so an agent from another vendor starts with no rules.
- **The graph was never built.** CLAUDE.md tells every agent to run `hexa graph context` and `hexa graph consumers`. On datum both failed for every agent, because nothing had run `hexa graph build`.
- **The lead and builder roles were datum's own.** datum's `.claude/agents/datum.md`, `datum-builder.md` and its `hexa-parallel-agents` skill hold rules that cost real failures to learn:
  - a push that failed `cargo fmt`;
  - a turn that ended while its remote run was still going;
  - test counts quoted from a local run instead of the pushed head;
  - two writers on one branch;
  - generated files edited by hand;
  - a CI runner restarted under a running job.

  Every other project starts without those rules.
- **No module said what it owns.** An agent placed in a crate or a layer could not tell what that module owns, what it may import, where its tests are, or which ADRs decided it.
- **Nothing said any of this was missing.** `hexa doctor` checks the install and inference. `hexa finish` checks git and the loop. Neither looks at whether a project can orient an agent.

The draft has seven items:

1. AGENTS.md.
2. A fresh graph.
3. Agent roles and the parallel-agents skill.
4. Module READMEs.
5. A remote `hexa do gate`.
6. Doctor and finish checks.
7. `hexa eval` scenarios.

This ADR decides items 1, 3, 4 and 6. Items 2 and 5 (graph freshness and its hooks, and `hexa do gate`'s remote mode) are being built at the same time in a sibling branch and are decided in that branch's ADR. Item 7 is recorded as a workplan, not decided here.

## Decision

1. **`hexa init` writes `AGENTS.md`, the project's one guide for every agent.** Its hexa-managed section sits between `<!-- hexa:agents-md:start -->` and `<!-- hexa:agents-md:end -->`. It holds, in this order:
   - **Start here:** `hexa status`, `hexa finish`, `hexa go`.
   - **Where builds run:** rendered from `.hexa/project.json`. That is `gate.full`; `test.remote` with "commit and push first", or "this machine" when it is unset; and the `test.runners` extensions.
   - **Find your place:** `hexa graph context <path>` before editing a file, `hexa graph consumers <path>` before deleting one, both after `hexa graph build`, and the module README.
   - **Several agents:** the lead and builder roles, and the parallel-agents skill.
   - **The shipped guide:** the verb table, the gate-first pipeline, the hexagonal layer rules, the file layout, and lessons through `hexa memory`.

   The shipped guide's source stays at `hexa-cli/assets/templates/claude-md-hexa-section.md`. Sealed tests read that path, so the file keeps its name and now feeds AGENTS.md. An AGENTS.md that already exists without the markers gets the section appended. Text outside the markers is never changed. The rules are `agents_md_section` and `where_builds_run` in `hexa-core/src/domain/orientation.rs`.

2. **CLAUDE.md imports AGENTS.md and keeps only what is Claude Code's.** Its managed section, between the existing `<!-- hexa:claude-md:start -->` and `<!-- hexa:claude-md:end -->` markers, becomes:
   - the line `@AGENTS.md`, Claude Code's import;
   - the Claude-only parts: what the hooks do, the skills, and the two agent definitions;
   - spawning builders with worktree isolation.

   Everything that AGENTS.md says is not repeated. `imports_agents_md` decides whether a CLAUDE.md imports it: a line that is exactly `@AGENTS.md`, outside a code fence.

3. **`hexa refresh` brings both managed sections up to date, in place.**
   - In each file it replaces only the bytes between the markers, through `replace_managed`. A second refresh changes nothing.
   - It creates AGENTS.md when the file is missing.
   - It appends the section to an AGENTS.md that has no markers.
   - It refuses a file whose end marker comes before its start marker, and names the file.
   - It upgrades a legacy CLAUDE.md as it does today.
   - It still never appends to a CLAUDE.md that has no section.

4. **hexa ships a lead role, a builder role and the parallel-agents skill as embedded assets.** They are `assets/agents/hexa-lead.md`, `assets/agents/hexa-builder.md` and `assets/skills/hexa-parallel-agents/SKILL.md`. They are project-neutral: built from datum's, with datum's hosts, crates, scripts and people removed. Between them they carry these rules:
   - run `cargo fmt --all` (or the project's formatter) before every push;
   - wait on your own remote runs, and never end the turn while one is running;
   - report test counts only from the remote run on the pushed head;
   - one writer per branch;
   - change generated files only through the tool that generates them;
   - never restart a CI runner that has a job in flight;
   - split work by file;
   - integrate, never trust: the lead reruns every gate itself.

   `hexa init` installs them into `.claude/agents/` and `.claude/skills/`. `hexa skill sync` writes the ones that are missing and names the ones whose copy differs. Only `--update` replaces those. A project's own copy, edited or under another name, therefore overrides: sync never overwrites it.

5. **Every module has a README that says what it owns, what it may import, where its tests are, and which ADRs decided it.**
   - **What `init --scaffold` writes.** It writes `README.md` into each layer directory it scaffolds, with four headings: `## Owns`, `## May import`, `## Tests`, `## ADRs`. The "may import" text is that layer's rule. The rule is `module_readme`.
   - **What `hexa docs check` flags.** It flags each module that has no `README.md` as a `missing_module_readme` warning. A project's modules are taken from the first of these that applies:
     - `docs.modules` in `.hexa/project.json`;
     - the members of a Cargo workspace;
     - the layer directories present under `src/`, `src/core/`, `internal/` and the root.
   - **What `hexa docs readmes` does.** It writes that skeleton into each module that lacks one, and never overwrites.

6. **`hexa doctor` and `hexa finish` report, per project, whether the project can orient an agent.** Each check that fails names the command that fixes it. The checks are:
   - **Hooks:** every hexa hook command in the template is present in `.claude/settings.json`. The fix is `hexa refresh`.
   - **AGENTS.md:** present, with a current managed section. The fix is `hexa refresh`.
   - **CLAUDE.md:** imports AGENTS.md, and its managed section is current. The fix is `hexa refresh`.
   - **Graph:** read from the `state` field of `hexa graph status --json`. `fresh` passes. `stale` or `missing` fails, and the fix is `hexa graph build`. If the verb is missing or prints something else, the graph is reported as `unknown`: listed, never a pass and never a failure. The rule is `graph_state`.
   - **Skills and agents:** every shipped file is present. If one is missing, the fix is `hexa skill sync`. A copy that differs is named as the project's own and does not fail.
   - **Module READMEs:** every module from §5 has one. The fix is `hexa docs readmes`.

   `hexa doctor` exits non-zero on any failing check. `hexa finish` lists each failing check as an `orientation` item, so `--exit-code` fails. Both run these checks only in an initialised project, one with `.hexa/project.json`.

## Consequences

- Any agent, from any vendor, starts from the same guide. CLAUDE.md shrinks to what only Claude Code reads.
- A project that falls behind the shipped guide, roles or skills is told so by `hexa doctor` and `hexa finish`, with the command that catches it up. Rollout: `hexa init --force`, then `hexa skill sync`, in each project.
- An existing CLAUDE.md that already carries a hexa section keeps its hand-written text. Its managed section becomes the import plus the Claude-only parts on the next `hexa refresh`.
- `hexa finish --exit-code` fails in an initialised project whose graph is stale. Builder B's SessionStart hook rebuilds the graph, so in practice that state is brief.
- hexa's own repository is held to the same rule for module READMEs: each workspace crate gets one.

## Implementation

**Gate:** `cargo test -p hexa-cli --test adr_2611221600_spec && cargo test -p hexa-core orientation`

Tests are written first, against temporary directories:

1. `init` on an empty directory writes AGENTS.md with its markers. CLAUDE.md imports it, with `@AGENTS.md` inside its managed section.
2. `refresh` on files that have hand-written text above and below each managed section, and a stale body, replaces only the managed bodies. Every other byte is unchanged, and a second refresh is a no-op.
3. `init` installs `.claude/agents/hexa-lead.md`, `.claude/agents/hexa-builder.md` and `.claude/skills/hexa-parallel-agents/SKILL.md`. Each carries the rules in §4. `skill sync` restores a deleted one and leaves an edited one alone.
4. `init --scaffold` writes a README with the four headings in every layer directory. `docs check` flags a module whose README was removed, and `docs readmes` writes it back.
5. `doctor`, in a project missing each piece, names each one with its fix command and exits non-zero. `finish --json` lists them as `orientation` items. A project with no `.hexa/project.json` gets none of them.
