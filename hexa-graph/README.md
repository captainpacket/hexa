# hexa-graph

## Owns

The code knowledge graph: extraction, `context_for`, `rank_lessons`, community detection. Builds and reads `.hexa/graph/graph.json`; its ports hold the extraction contract.

## May import

No other hexa crate. `hexa analyze .` enforces the layer rules.

## Tests

`cargo test -p hexa-graph`. Before changing a file here, `hexa graph context <path>`; before deleting one, `hexa graph consumers <path>`.

## ADRs

ADR-2606071340 (the excision oracle).
