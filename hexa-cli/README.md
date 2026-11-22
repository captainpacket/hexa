# hexa-cli

## Owns

The binary. Every verb is a primary adapter (`src/commands/`); it composes the crates. The embedded assets (`assets/`: skills, agent roles, templates, scaffolds) are what `hexa init` installs.

## May import

Every hexa crate, through their ports and use cases. A verb never constructs another crate's adapter; it asks the crate's composition root. `hexa analyze .` enforces the layer rules.

## Tests

`cargo test -p hexa-cli` (integration tests in `hexa-cli/tests/`, one file per sealed spec where there is one). Before changing a file here, `hexa graph context <path>`; before deleting one, `hexa graph consumers <path>`.

## ADRs

ADR-2611221600 (agent orientation), ADR-2611161800 (`hexa finish`), ADR-2611160730 (compliance by refusal).
