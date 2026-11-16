# ADR-2611160750: Leases follow the architecture

**Status:** Proposed
**Date:** 2026-11-16
**Drivers:** Agents developing in parallel keep overrunning each other. Asked: can hexa use the hexagonal layers to isolate agents, so that work in use cases is kept apart from work in the domain?
**Supersedes:** none. Builds on the task ledger (`hexa agent register/report/release`, `hexa dev worktree merge-order`) and ADR-2611160730 (compliance by refusal).

## Context

- hexa already leases paths. `hexa agent register --task <t> --lease <path>…` refuses a lease that overlaps another task's. A session is held to its lease when `HEXA_TASK=<id>` is set or its agent_id equals `--owner`. An unbound session is not constrained.
- `hexa dev worktree merge-order` merges finished tasks one after another.
- Three gaps:
  1. Nothing makes an agent register, so most sessions are unbound.
  2. A lease is a list of paths, unrelated to the architecture `hexa analyze` already knows.
  3. Overlap is judged by path alone, so two leases that touch no common file but depend on each other's contracts both pass and break each other.
- The overruns actually seen on 2026-11-16 were mostly not layer collisions. They came from sessions sharing one checkout: harden resetting HEAD, and `do run` sweeping in other people's edits. That is ADR-2611160730.

## Decision

Make the architecture the lease unit, and treat the inner layers as contracts.

1. **A lease can name a layer slice.** `--lease usecases/billing` or `--lease adapters/secondary/postgres` resolves through the analyzer's layer map to the files it covers. Plain paths still work.
2. **Overlap includes dependency, not just paths.** A lease on `ports/` or `domain/` is a *contract lease*. `register` refuses one whose files are used by code under another live lease (found with `hexa graph consumers`, after `graph build`), unless the contract is explicitly frozen for the other lease.
3. **Outer layers work against frozen contracts.** A use-case or adapter lease records the port and domain files it depends on, read from the graph, as *frozen for the duration*. An edit to a frozen file from any other lease is refused by the hook. Adapters never import adapters, so adapter leases do not conflict with each other through dependency.
4. **Merge inner to outer.** `merge-order` sorts finished tasks: domain, then ports, then use cases, then adapters, then the composition root. A later task rebases on the earlier ones and must re-pass its gate.
5. **Hot shared files get short leases.** Files every layer touches are leased for the length of one gated edit, through `hexa do gate`, and never for a whole task. That covers the composition root, module registrations such as `mod.rs` and `lib.rs`, manifests and lockfiles.
6. **Registration is the default for parallel agents.** When the hooks see a second live session in the same repository, an edit from an unbound session is refused with the command to register. A single session working alone is not asked.
7. **One worktree per leased task, one shared ledger.** That is already how the ledger works; this makes it the norm, not an option.

## Consequences

- Parallel agents in different adapters, or in use cases against frozen ports, run without stepping on each other. A change to a contract has to wait for, or be agreed with, its consumers. That is the coupling the architecture already declares.
- Work that is not hexagonal gets no help from layers. A large single-file module, such as the Lintel renderer `app.js` in harness, has one lease at a time. The fix there is splitting the module along its seams, not a smarter lock.
- Contract leases depend on a fresh graph. A stale graph must fail loudly (ADR-2611160730 §7), never pass quietly.
- More refusals at `register` time. That is the intended trade: a conflict found before the work, not after the merge.

## Implementation

Gates first, each a failing test against a temp repo with a hexagonal layout:

1. `register --lease usecases/billing` resolves to that slice's files.
2. A lease on `ports/billing.rs` is refused while a live lease on `usecases/billing` uses it, and the refusal names the consumer.
3. With a use-case lease live, an edit to its frozen port from another session exits 2 in `pre_edit` and `pre_bash`.
4. `merge-order` puts the domain task before the use-case task before the adapter task.
5. A second live session makes an unbound edit refuse, with the `register` command in the message.

## References

- ADR-2611160730: compliance by refusal, not by interface. It covers the hooks, `do gate` and harden safety this builds on.
- ADR-2610221834: harden fixes in parallel worktrees.
- ADR-2610271649: an adapter importing another adapter in its own layer is a violation.
- `hexa agent register --help`, `hexa dev worktree merge-order`.
