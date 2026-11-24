---
name: hexa-git-effects
description: What each hexa verb does to git — what it commits, on which branch, what it resets or reverts, whether it needs a clean tree, whether it can run beside another session, and how to recover. Read before running do run, harden, build, spec tests, bench, gate coverage or design adr in a tree that holds other work. Use when the user asks "what will this commit", "is it safe to run harden now", "hexa lost my change", "undo a hexa commit", or "git reflog".
---

# hexa git effects — what each verb does to your repository

Several hexa verbs commit, and some reset. None of them stashes your work
first. Read the row for a verb before running it in a tree that holds edits of
yours, or in a checkout another session is using. "This branch" means the
branch checked out where you run the verb.

Rules that hold for every verb below:

- **Start from a clean tree.** `git status` shows nothing tracked as modified.
  Commit your own work first, even as a throwaway WIP commit.
- **One committing verb per checkout at a time.** Only `hexa do run` takes a
  lock, and it is held only against other `hexa do` runs. For parallel work
  give each session its own worktree (`hexa dev worktree`).
- **Every commit and every reset is in the reflog.** `git reflog` lists where
  HEAD was before each one, and `git reset --hard <sha>` or
  `git cherry-pick <sha>` brings it back. An *uncommitted* edit a verb reverts
  is not in the reflog: commit before you run.

## The table

| Verb | Commits | Branch | Resets or reverts | Clean tree | Beside another session |
|---|---|---|---|---|---|
| `hexa do run` | `--file` and each `--also`, only if the evidence passes | this branch | on a failed run, `--file` and `--also` go back to their bytes from before the run | refuses when other tracked files in the package are modified (see below) | not with harden, build or a person editing the same files; serialised with other `hexa do` runs |
| do gate | only the files it names | this branch | nothing | — | — |
| `hexa harden` | each fix, as the files that changed while the fixing agent ran | this branch | yes: soft reset, file reverts, `reset --keep` plus cherry-pick under `--fast-gate` | not checked | **no** |
| `hexa build` | every file that was clean before the build and changed by the end | this branch | nothing | not checked; files already modified are left out | **no** |
| `hexa spec tests` | the generated test file only | this branch | nothing on your tree; temporary worktrees are removed | not checked | refuses if HEAD moves while it runs |
| `hexa bench agentic` | an oracle commit per fixture, on a branch it makes | **switches this checkout** to `bench/<time>`, then back | `reset --mixed HEAD~1` per fixture; deletes every new `hexa/auto/*` branch and worktree | not checked | **no** |
| `hexa gate coverage` | nothing | — | deletes each module in the working tree, runs the gate, writes it back | not checked | **no**: it deletes files you may be editing |
| `hexa design adr` | nothing (writes the ADR file and `.hexa/loop.json`) | — | nothing | the draft must be committed and unchanged | yes |

## Each verb

### `hexa do run`

- **Commits** on this branch, only when the evidence passes:
  - `--file`;
  - each `--also` path that exists after the run (a new one is added);
  - nothing untracked that was not named.
  - The message is `feat(direct): <task>`. With `--uncounted` the subject
    begins `[evidence uncounted]`.
- **Dirty files.** Before ADR-2611160730 §2, `do run` also committed every
  tracked file that was already modified in the target's package. The package
  is the nearest directory holding a `Cargo.toml` or `package.json`, so with a
  root `Cargo.toml` it was the whole repository. Now it refuses to start while
  such a file is modified. Name it with `--also` to commit it in the same
  commit, or pass `--include-dirty` to sweep the package in on purpose. The
  refusal comes before any model call, so nothing is spent. Start clean.
- **Reverts** `--file` and `--also` to their pre-run bytes when a run fails
  or times out, which overwrites anyone else's edits to those files. Other
  files a delegate changed are left on the tree and named. `--fast` does not
  revert a failed attempt.
- **Lock:** `<git-common-dir>/hexa-exec.lock`, held only against other `hexa do`
  runs.
- **Recover:** the commit hash is printed. Run `git revert <sha>`, or
  `git reset --soft HEAD~1` while it is still the tip, then unstage what
  should not have been in it.

### `hexa do gate`

ADR-2611160730 §3: gate a patch you wrote yourself.
For example, `hexa do gate --files src/a.rs src/b.rs -e "cargo test -p mycrate" -m "fix the parser"`.

- **Calls no model.** It runs the evidence on the tree as it is.
- **Commits** only `--files`, on this branch. The message is yours, with a
  `Gate:` line that records the evidence.
- **Refuses before running anything** when:
  - no file is named, or a path escapes the repository;
  - a file is a credential;
  - a named file has no change against HEAD;
  - a sealed test or spec record changed.
- **A failing, vacuous (0 tests) or unrecognised result commits nothing** and
  leaves your patch exactly where it is. Use `--uncounted` when the evidence is
  not a test runner.
- **`--expect-red`** (a tests-first commit) commits only if the gate fails on
  test assertions with at least one test run. A pass, zero tests or a compile
  error commits nothing. A red commit never lands.
- **Under `test.remote`** (unless `--local`) it pushes a candidate commit to
  `refs/heads/hexa-gate/<id>` on the branch's upstream and deletes that ref
  afterwards, whatever the outcome. On a verdict it accepts, it moves this branch
  to the tested commit, pushes it, reads it back with `git ls-remote`, and adds
  a git note under `refs/notes/hexa-gate`. A refused push (`push-refused`) leaves
  the commit on this branch here, unpushed.
- **`--dry-run`** pushes, runs and commits nothing. It prints the plan.
- **Resets nothing, reverts nothing.**
- **Recover:** `git reset --soft HEAD~1` while it is still the tip.

### `hexa harden`

Run it only in a clean checkout no one else is using. A worktree is best.

- **Commits** each verified fix on this branch, as `fix(harden): <title>`
  (author `hexa-factory`). The fix is the set of files whose bytes changed
  while the fixing agent ran, which can be up to 15 minutes. **Anything anyone
  else changed in that window is committed as part of the fix.** A fix that
  touches a file that was already modified when the pass began is left
  uncommitted.
- **Resets:**
  - If HEAD moved during a fix, whoever moved it, harden runs
    `git reset --soft` back to where it started. Another session's commit is
    then folded into the next `fix(harden)` commit.
  - When a fix fails its gate, or touches a sealed test, the changed files are
    written back to their old bytes, checked out from HEAD, or deleted. That
    includes **another person's uncommitted edits**, and no copy is kept.
  - With `--fast-gate`, fixes are batched. A failing batch is bisected with
    `git reset --keep` and cherry-pick from the batch's base. **A commit made
    by anyone else since that base, and not part of the batch, is dropped.**
- **Recover:** find HEAD from before the run in `git reflog` and cherry-pick
  what was lost. Fixes that were set aside or dropped are listed in the run's
  report with their SHAs. A reverted uncommitted edit cannot be recovered.

ADR-2611160730 §1 makes harden refuse a dirty tree and take a per-repo lock.
Until your binary does that, these rules are yours to keep.

### `hexa build`

- **Commits** once at the end on this branch, as `feat: <challenge>` (author
  `hexa-factory`). The commit holds every file that was clean when the build
  started and changed by the end, **including edits someone else made in that
  time**. Files already modified at the start are left out. `PROVENANCE.md` is
  written afterwards and not committed.
- **Resets** nothing itself. `--harden` then runs a harden pass, with
  harden's soft reset.
- **Recover:** `git reset --soft HEAD~1` and re-commit what was yours.

### `hexa spec tests`

- **Commits** only the generated test file (plus a `git rm` of a test file it
  replaces), on this branch, as
  `test(spec): <ADR> scenarios, generated from the accepted spec (failing)`.
  The spec JSON is written and not committed: commit it yourself. `--only`
  commits the test file, the spec JSON and the replaced files together.
- Each red check runs in a temporary detached worktree, removed afterwards.
  `--red-at` records its proof under `refs/hexa/red/<ADR>` without moving
  your branch.
- **Refuses** if HEAD moves while it runs. If it fails before committing, it
  removes the file it wrote.
- **Recover:** `git revert <sha>` of the `test(spec)` commit.

### `hexa bench agentic`

Run it only in a checkout no one else is using.

- Runs `git checkout -b bench/<time>` **in this checkout**. Refuses only on a
  detached HEAD.
- For each fixture:
  - It writes and stages the oracle files, then runs `git commit` with no
    pathspec, which **commits anything else that was staged**.
  - It runs `do run` in an isolated worktree, then force-deletes **every
    `hexa/auto/*` worktree and branch created during the run**, another
    session's included.
  - It undoes the oracle commit with `git reset --mixed HEAD~1` and deletes
    the oracle files.
- At the end it checks out your original branch, deletes `bench/<time>` and
  prunes worktrees.
- **Recover:** `git reflog` shows the original branch tip and the bench
  commits. `git checkout <branch>` returns you to your branch.

### `hexa gate coverage`

- Commits nothing and runs no git command. For each candidate module it
  **deletes the file in your working tree**, runs the gate, and writes the
  original bytes back, even when the gate panics.
- A killed run (SIGKILL, a power cut) leaves a module deleted. A restore
  overwrites any edit made to that file meanwhile.
- **Recover:** `git status`, then `git checkout -- <file>` for each deleted
  module.

### `hexa design adr`

- Refuses a draft that is untracked, uncommitted, or changed since its
  review. Writes the proposed ADR under `docs/adrs/` and updates
  `.hexa/loop.json`. **Commits nothing**: review the ADR and commit it.

### Also: verbs that write and do not commit

`hexa loop stage done` appends evidence to the ADR and records the HEAD sha,
noting "with uncommitted changes" when the tree is dirty. `hexa spec accept`
writes the spec JSON. Neither commits. Commit the files they wrote.

## When something went wrong

1. Stop every hexa run in that checkout.
2. `git reflog -20`: find the last entry before the run started.
3. `git status` and `git stash list`: hexa never stashes, so an entry there is
   yours or another session's.
4. Lost commit: `git cherry-pick <sha>`. Unwanted commit at the tip:
   `git reset --soft HEAD~1`. A branch that moved under you:
   `git reset --hard <sha-from-reflog>`, after saving anything uncommitted.
5. Record what happened, so the next run is safer:
   `hexa gap add <id> --what "<…>" --around "<step>" --fix "<path> — <change>"`.
