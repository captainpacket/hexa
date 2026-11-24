# hexa-core

## Owns

The contract surface: the inference port and its mock, message, tool and validation types, value types, and the ports every adapter reaches the domain through (`ports::edit`, `ports::spec`, `ports::provenance`, `ports::orientation`, …). Zero runtime dependencies: nothing can bleed a runtime concern upward.

## May import

Nothing else in the workspace. `domain/` imports only `domain/`; `ports/` imports `domain/` for value types. `hexa analyze .` enforces the layer rules.

## Tests

`cargo test -p hexa-core` (unit tests beside the code, and `hexa-core/tests/`). Before changing a file here, `hexa graph context <path>`; before deleting one, `hexa graph consumers <path>`.

## ADRs

ADR-2611221600 (agent orientation's domain rules), ADR-2611191120 (test runners), ADR-2611161830 (AG-UI events), ADR-2610221540 (specs).
