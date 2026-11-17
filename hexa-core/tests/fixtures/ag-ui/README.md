# AG-UI schema (vendored)

`schema-draft-1.1.json` is the AG-UI protocol's JSON Schema, draft version 1.1, fetched on 2026-11-16 from
https://docs.ag-ui.com/spec/draft/schema.json. It is the oracle the `agui_` tests validate hexa's events against
(ADR-2611161830 §9): hexa is checked against the protocol's own definition, not against our reading of it.

Source: the AG-UI project, https://github.com/ag-ui-protocol/ag-ui, under the MIT License,
Copyright (c) the AG-UI contributors. Used unmodified, in tests only; nothing here ships in the binary.

When the draft is frozen, replace this file with the frozen version and move `agui::PROTOCOL_VERSION` with it.
