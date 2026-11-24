# hexa-exec

## Owns

The agent loop, per-run worktree isolation, the adversarial harness, transcript compression, the guarded tool library and the file-backed local store. Everything the loop touches is behind a port in `src/ports.rs`; `default_deps()` in `lib.rs` wires the defaults.

## May import

`hexa-core`, `hexa-infer`, `hexa-graph`, `hexa-git`. Only `lib.rs`, the crate's composition root, imports an adapter. `hexa analyze .` enforces the layer rules.

## Tests

`cargo test -p hexa-exec`. Before changing a file here, `hexa graph context <path>`; before deleting one, `hexa graph consumers <path>`.

## ADRs

ADR-2610221540 (sealed specs), ADR-2610221834 (harden fixes in parallel worktrees), ADR-2611020815 (a run does not replace the gate).
