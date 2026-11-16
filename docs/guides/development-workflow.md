# Development workflow

The pipeline has six steps. A gate is written before the code and is not derived
from it. That order is the whole method.

```
Decide → Gate → Diverge → Build → Harden → Ship
```

## 1. Decide

A change that adds a port, an adapter, or a dependency gets an ADR in
`docs/adrs/`. The ADR records the decision and the reasoning. It is history and
is allowed to be prose, because it never claims to describe the present.

```bash
hexa adr schema            # the template and the next number
hexa adr list              # every ADR and its status
hexa adr search <term>
hexa adr accept <id> -r "<why>"  # Proposed → Accepted
```

ADRs are append-only. A changed decision gets a new ADR that supersedes the old
one.

## 2. Gate

Write the command that must exit 0. It is the only authority on whether the work
is finished, so it comes before the code, and it is not generated from the code.

```bash
cargo test --test bookmarks       # a test that does not exist yet
```

A gate that passes having run nothing is rejected. `hexa do` and `hexa scaffold`
both check for a vacuous pass.

## 3. Diverge

`hexa scaffold` and `hexa build` propose several designs and red-team each one
before building. The spec they synthesize is a disposable intermediate. It is
not kept and not maintained.

```bash
hexa scaffold "<what to build>" --target <dir> --lang rust --designs 2
hexa build "<subsystem>" --target <dir> --gate "<cmd>" --designs 3
```

## 4. Build

The frontier path writes the code. The gate decides whether it counts. For a
bounded change to one file:

```bash
hexa do run "<task>" --file <path> --evidence "<cmd>"
```

hexa edits, runs the command, and commits only on exit 0. A failed edit is
reverted, so each attempt starts from the original file.

For a scaffolded project there is a second gate. `hexa scaffold --grade A` fails
the build if the architecture grade falls below it or any import crosses a
boundary. In CI, gate on `hexa analyze . --grade A --exit-code`: `--grade` alone
is a letter floor, and one violation still grades A.

## 5. Harden

The adversarial pass hunts the result for bugs its own tests missed. Each
finding is verified skeptically before any edit, and each fix is gated.

```bash
hexa harden <path> --gate "<cmd>"
hexa build "<subsystem>" --target <dir> --gate "<cmd>" --harden   # both in one
```

On this project's rate limiter it found three real bugs behind fourteen passing
tests. One was a `u128` overflow accepted on a confidently wrong comment.

## 6. Ship

```bash
hexa analyze . --exit-code     # grade and violations, nonzero on any
hexa graph consumers <path>    # before deleting anything
cargo test --workspace
```

Then commit. The gate has already decided the work is done. The commit records
it.

## Several agents at once: the task ledger

When more than one agent edits the same repository, hexa keeps the record and
the rules (ADR-2611051850). It does not start, supervise or stop an agent; the
harness owns the processes.

```bash
hexa status                      # runs, tasks and subagents in progress, last outcomes (JSON when piped)
hexa agent register --task api --lease src/api/ --gate "cargo test -p api"   # prints the task id
export HEXA_TASK=<id>            # this session may now edit only src/api/
hexa agent report --id <id> --outcome passed --commit $(git rev-parse --short HEAD)
hexa agent list                  # the ledger; `hexa agent release --id <id>` frees a crashed task
hexa dev worktree merge-order    # finished tasks' worktrees, in the order to merge them
```

- The ledger is `.hexa/tasks.json` in the main checkout, found from any
  worktree, and git-ignored.
- Two tasks may not hold overlapping leases. A refusal is a JSON object with
  `code`, `message`, `hint`, `field` and `retryable`.
- `hexa hook pre-edit` refuses an edit outside the lease (exit 2, naming the
  task) for a session bound by `HEXA_TASK` or by a Claude Code `agent_id` equal
  to the task's `--owner`. An unbound session is unchanged. A lease governs
  edits made through hooked tools, not a shell redirect.
- Sealed tests and credential-looking paths (`.runner`, `.env*`, `*.pem`,
  `*.key`, `id_rsa*`, `*.p12`, `credentials*`, `.netrc`, `*token*`, `*secret*`)
  are never leasable, and `hexa do` never commits them. `hexa do` commits the
  `--file` and the `--also` paths only; a file that was already dirty beside it
  stays out. `hexa do run --plan-only` shows the list without a model.
- Every write verb takes `--dry-run`. The `SubagentStart` and `SubagentStop`
  hooks bind and close tasks; whether a live Claude Code sends them in the
  documented shape is not yet verified.

## What is not in the pipeline

There is no spec step. A spec that cannot be run does not exist. It becomes a
gate, or it becomes ADR prose.

Every verb runs in-process and exits. Nothing runs between invocations, and
nothing has to be started before one.
