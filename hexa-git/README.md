# hexa-git

## Owns

Git plumbing over libgit2: history and worktrees.

## May import

No other hexa crate. `hexa analyze .` enforces the layer rules.

## Tests

`cargo test -p hexa-git`. Before changing a file here, `hexa graph context <path>`; before deleting one, `hexa graph consumers <path>`.

## ADRs

ADR-2611161800 (a plan ends on main).
