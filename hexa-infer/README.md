# hexa-infer

## Owns

Every inference adapter (`src/adapters/secondary/`), the endpoint registry and tier resolution. The only crate that may name a provider or a model (founding goal G1).

## May import

`hexa-core`. Adapters reach the core through its ports; `wiring.rs` is the composition root that picks the adapter serving a model. `hexa analyze .` enforces the layer rules.

## Tests

`cargo test -p hexa-infer`. Before changing a file here, `hexa graph context <path>`; before deleting one, `hexa graph consumers <path>`.

## ADRs

ADR-2610281516 (inference is measured, cached and repaired), ADR-2610270901 (claude text as a tool-less completion).
