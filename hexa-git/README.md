# hexa-git

## Owns

Git plumbing over libgit2 and the git CLI: history, worktrees, and which tags hold a commit.

## May import

No other hexa crate. `hexa analyze .` enforces the layer rules.

## Tests

`cargo test -p hexa-git`. Before changing a file here, `hexa graph context <path>`; before deleting one, `hexa graph consumers <path>`.

## ADRs

ADR-2611161800 (a plan ends on main). ADR-2611250845 (the release a feature shipped in: `release.rs`).
