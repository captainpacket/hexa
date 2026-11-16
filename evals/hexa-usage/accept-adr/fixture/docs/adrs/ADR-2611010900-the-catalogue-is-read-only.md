# ADR-2611010900: The catalogue is read-only

**Status:** Proposed
**Date:** 2026-11-01
**Drivers:** Two adapters were about to write to the catalogue through different paths.

## Context

The catalogue is loaded once and browsed. Nothing in the product edits it.

## Decision

1. The `Catalogue` port has no method that writes.
2. A change to the titles is a new catalogue, built by the composition root.

## Consequences

- Browsing needs no locking.

## Implementation

**Gate:** `cargo test`

## References

- none
