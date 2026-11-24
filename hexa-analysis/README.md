# hexa-analysis

## Owns

Boundary checking with tree-sitter, the one layer classifier and the project's layer map, cross-package import resolution, dead-export and cycle detection, the coverage ceiling, rule conformance and the health detectors. Powers `hexa analyze`.

## May import

No other hexa crate. `default_ast()` in `lib.rs` wires the tree-sitter adapter behind `AstPort`. `hexa analyze .` enforces the layer rules.

## Tests

`cargo test -p hexa-analysis`. Before changing a file here, `hexa graph context <path>`; before deleting one, `hexa graph consumers <path>`.

## ADRs

ADR-2609241707 (the grade is capped at the classified share).
