# ADR-2611221610: The code graph is always built and fresh, and `hexa do gate` runs where the tests run

**Status:** Accepted
> 2026-11-22 Accepted: the owner asked for it (gary, 2026-11-22: "how do we make sure that the agents know how to use hexa effectively for every project?"); the workplan draft `draft-2611221546-every-hexa-project-gets-agent-0669d0` items (2) and (5). Accepted under his standing instruction to accept the ADRs and specs and land the work, recorded by the implementing agent (builder B) on his behalf
**Date:** 2026-11-22
**Drivers:** datum, 2026-11-22. The umbrella decision is builder A's "Every hexa project gets agent orientation" (id 2611221600, on branch `t3/agent-orientation-init`; named without its prefix here until both land, so the citation checker does not look for a file this branch lacks). It decides the rest of that draft: AGENTS.md, roles, skills, module READMEs and `hexa doctor`, and it says items (2) and (5) are decided here. `hexa doctor` reads the `state` field of `hexa graph status --json`, which this ADR decides.
**Supersedes:** none. Amends ADR-2611191120 §6 (`hexa do gate` under `test.remote` now runs remotely for a branch that tracks a remote branch, and refuses only one that does not) and ADR-2610221540 §6 as `hexa do gate` reads it (an amendment `hexa spec amend` recorded is not a broken seal).

## Context

**The graph is often missing, and when it is built it can be wrong.** CLAUDE.md tells every agent to run `hexa graph context <path>` before an edit and `hexa graph consumers <path>` before a deletion. On datum the graph had never been built, so both commands failed for every agent ("no graph … Build it first: `hexa graph build`"). Nothing builds the graph except that verb: `hexa init` does not, and no hook does. When it was built, a stray worktree inside the checkout (`./origin/t3/…`, a full second copy of the tree) doubled every node, 35,288 instead of 17,644. The walk in `hexa-graph/src/lib.rs` skips only a fixed list of directory names. It reads gitignored files and other repositories' files alike. Staleness is noticed only as a warning on read (`newer_sources`), and only for files newer than the graph. A deleted file, or a branch switch that restores older mtimes, goes unnoticed.

**`hexa do gate` cannot run in a project whose tests run on build servers.** The commit hook refuses a hand `git commit` of source and points to `hexa do gate`. Under `test.remote`, `hexa do gate` refuses unless `--local` is given (ADR-2611191120 §6), and on datum `--local` means building on a Mac that cannot build it. It also refuses every sealed test and every spec record that differs from HEAD (`broken_seals` in `hexa-exec/src/direct_exec.rs`). That includes the edit `hexa spec amend` has just recorded, so a recorded, reviewed amendment cannot be committed through the gate either. On 2026-11-22 datum's builders went around the gate with git plumbing at least four times. A gate that every honest path has to bypass teaches agents to bypass it.

## Decision

1. **The graph reads the project's files only.** `hexa_graph::source_files(root)` lists the files a build reads, sorted. In a git checkout these are the files git would track at `root`: tracked, plus untracked files that are not ignored. No file inside a directory below `root` that holds its own `.git` (a nested repository or worktree) is read. Outside git, the walk skips such directories too. The fixed list of excluded directory names (`target`, `node_modules`, `.hexa`, `.claude` and the rest) still applies in both cases. `build` and the staleness check both use this list.

2. **A build records what it read.** `hexa graph build` writes the graph atomically (to a temporary file, then renamed) and writes `.hexa/graph/stamp.json` beside it: `built_at`, `nodes`, `edges`, and `fingerprint`. The fingerprint is `hexa_graph::tree_fingerprint(root)`: a sha256 over every file `source_files` lists, with its path, size and modification time. An edit, an added file, a deleted file and a checkout of another branch each change it. Computing it costs one `git ls-files` and one `stat` per file. Nothing is parsed.

3. **`hexa graph status [--json]` says whether the graph can be trusted.** `state` is `missing` (no graph file), `stale` (no stamp, or the fingerprint differs from the tree's), or `fresh`. The JSON is `{"state", "built", "fresh", "built_at", "nodes", "changed"}`, where `changed` is the number of source files modified since `built_at`. The verb exits 0 in every state: it reports, and `hexa doctor` decides. The fix it names is `hexa graph build`.

4. **`hexa init` builds the graph.** After it writes the project, init builds the graph for the target and prints one line saying so. A build failure is printed and does not fail init.

5. **The SessionStart hook rebuilds a missing or stale graph in the background.** It prints one line, `graph: <state> — rebuilding in the background` or `graph: fresh (<n> nodes)`, and starts `hexa graph build` detached, with output to `.hexa/graph/build.log`. A lock file, `.hexa/graph/build.lock`, stops a second rebuild from starting while one younger than 15 minutes holds it. A fresh graph is not rebuilt. The session never waits for a build.

6. **The PreToolUse edit hook prints the file's graph context, compact and bounded.** Before an edit to a project file, when a graph exists, the hook prints `hexa graph context` for that file: at most 12 lines, with at most 5 items per list (defines, imported by, used by). It does this once per file per session. A stale graph is named as stale in the first line. With no graph, or a file the graph does not hold, the hook prints nothing. It never blocks an edit.

7. **Under `test.remote`, `hexa do gate` runs the gate remotely, and commits only what passed there.** Without `--local`:
   - It checks the named files as it does locally (in the repository, not credentials, changed, seals intact under §8).
   - The branch must track a remote branch. One that does not is refused: the refusal names `git push -u origin <branch>` and `--local`, and nothing runs. So ADR-2611191120 §6's refusal still holds for a branch that was never pushed.
   - It builds the candidate commit without moving the branch: HEAD's tree with the named files as they are in the working tree, HEAD as its parent, and the given message plus `Gate: <cmd> (remote: test.remote …)`.
   - It pushes the candidate to `refs/heads/hexa-gate/<first 12 of its id>` on the remote of the branch's upstream, and runs the gate through `test.remote` for that commit (ADR-2611191120 §3, §5). The output must name the full commit id, or the run is a runner failure.
   - On a pass, read by the evidence reader as for a local gate, the branch moves to the candidate itself, the commit that was tested. The move is a compare-and-swap against the HEAD the candidate was built on. The index of the named files is synced, and the branch is pushed to its upstream (never forced). The remote output's last 30 lines are the evidence: printed, recorded in the run log, and attached to the commit as a git note under `refs/notes/hexa-gate`. Landing then proceeds as for a local gate.
   - On a failure, a vacuous run or a runner failure, nothing is committed, the branch does not move, and the change stays in the tree. The output's tail is printed.
   - The temporary ref is deleted in every case.

8. **A sealed file whose amendment `hexa spec amend` recorded is accepted by `hexa do gate`.** A sealed file that differs from HEAD's seal is accepted when the spec record in the working tree meets all of the following:
   - It differs from HEAD's copy only in its `sealed` map and by amendments appended to `amendments`. HEAD's amendments are a prefix of the new list.
   - The new amendments, applied in order, carry this file's seal from HEAD's hash to the file's current sha256.
   - Its `sealed` map names that hash.
   
   Then that record is accepted too. Both the file and the record must be among `--files`, so they are committed together. Anything else is refused as before: an edit with no amendment, an amendment whose `to` is not the file's content, a record changed in any other field, or a record not named. `hexa do run`, where an agent edits, is unchanged and still refuses every changed seal.

9. **The commit hook's refusal names the remote gate.** When `test.remote` is set, the hook's refusal of a hand `git commit` of source still names the `hexa do gate` command. It adds that the gate runs on the remote runner (test.remote) and commits only on exit 0. It no longer leaves an agent to infer that `do gate` would refuse.

## Consequences

- Every hexa project has a graph from `hexa init` on, and each session starts by refreshing it when the tree has moved. "No graph" stops being the normal state, and `hexa doctor` can name it.
- A checkout holding another worktree, or a gitignored build tree, no longer inflates the graph.
- The edit hook costs one graph load per file per session, and it prints nothing when there is no graph.
- A remote `do gate` costs one push and one remote run, as `hexa spec verify` already does. Each one leaves a temporary branch on the forge for as long as the run takes.
- The amendment path is end-to-end: `hexa spec amend` records the change and shows the diff, and `hexa do gate` commits the record with the test.

**Gate:** `cargo test -p hexa-cli --test adr_2611221610_spec`
