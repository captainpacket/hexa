# ADR-2611250845: `hexa adr features` lists every feature the decisions name, with where each stands

**Status:** Accepted
> 2026-11-25 Accepted: the owner asked for it (gary, 2026-11-25, in datum: "we need a definitive fact sheet on all features, so we can manage the product lifecycle", then "we should be able to extract a feature from an ADR than lists that feature in a table relevant to proposed, accepted or completed, this should be in hexa", then "start the hexa features verb now"). Recorded by the implementing agent on his behalf. The spec is drafted, not accepted: `hexa spec accept ADR-2611250845` is his.
**Date:** 2026-11-25
**Drivers:** datum, 2026-11-25. Its owner asked "where is our feature list verified" and there was no one answer: the release scope is one manifest, the admin's switches another, `datum describe` a third, and the data sheet and the trailer make claims nothing ties to the code. Every feature datum has was decided in an ADR (176 of them), and `hexa adr board` already reads from each ADR's text how far it is built (ADR-2611151030). What is missing is the unit a product is managed by. An ADR is a decision; one ADR may give three features and another none (a CI rule, a naming rule).
**Supersedes:** none. Builds on ADR-2611151030 (the board): a feature's state is the board's reading of its ADR, never a second opinion.

## Context

- **The board answers "is this decision built", per ADR and per phase.** `domain::adr_board::analyze` returns a `State` (`proposed`, `accepted`, `building`, `built`, `retired`, `unknown`) and the phases with `built` on each, from `**Built:**` lines and `## Phases`. It names no feature.
- **A feature is narrower or wider than an ADR.** One of datum's decisions (business applications) gives "applications defined by their flows" and "the application impact read"; another (how its CI is laid out) gives nothing a user would call a feature. A list made of ADR titles is neither complete nor readable.
- **A list kept by hand beside the ADRs goes stale.** datum's data sheet did (its `lesson:datasheet-drift`). The list has to be read from the record that already changes when the work does.
- **The owner named three states: proposed, accepted, completed.** They are the board's states seen from a feature: decided or not, built or not.

## Decision

1. **An ADR names its features in a `## Features` section holding one table.** The first table in that section, outside any code fence, is read. Its header row names the columns; the match ignores case.
   - `Feature` is required: the feature's name, as a person would say it.
   - `What it does` (or `What`) is optional: one sentence.
   - `Phase` is optional: the id of the phase of this ADR that builds the feature (`R2`), or several separated by commas or spaces. Empty means the whole ADR.

   ```
   ## Features

   | Feature | What it does | Phase |
   |---|---|---|
   | Blast radius | Says what goes down with a device before it is changed | R2 |
   ```

   An ADR with a `## Features` section and no table rows declares that it gives no feature (a process or tooling decision). That is a statement, and it is different from saying nothing.

2. **A feature's state is one of four, taken from the board's reading of its ADR (`domain::adr_board::analyze`).**

   | State | When |
   |---|---|
   | `proposed` | The ADR is proposed and what builds the feature is not built. |
   | `accepted` | The ADR is accepted and what builds the feature is not built, or only part of it is. |
   | `completed` | What builds the feature is built: every phase the row names, or the whole ADR when it names none. |
   | `retired` | The ADR is superseded, rejected, deprecated or abandoned. |

   "Built" is the board's evidence (`**Built:**` lines, a `completed` status), never the status word alone. A feature whose row names two phases with one built is `accepted`, with `built` saying `1 of 2`. An ADR whose status cannot be read gives its features the state `unknown`.

3. **An ADR with no `## Features` section is `undeclared`, and still appears.** It gives one row: the ADR's title as the feature's name, the ADR's state by rule 2, and `declared: false`. The list is therefore complete on the first run in any project, and says how much of it is a title standing in for a feature. A retired ADR with no section gives no row.

4. **The rules are a pure function of ADR text, in `hexa-core/src/domain/adr_features.rs`, reached through `hexa-core/src/ports/adr_features.rs`.** `features_of(id, title, status, content) -> AdrFeatures` reads one ADR. It calls `adr_board::analyze` for the state and reads only the `## Features` section itself. Nothing in it touches a file, git or a model.

5. **`hexa adr features` prints the table.** JSON when stdout is not a terminal or `--json` is given; text otherwise; `--md` for a Markdown fact sheet.
   - Each row: `feature`, `what`, `state`, `adr`, `adr_title`, `phases`, `built` (`done` and `total`), `declared`, and `released` (decision 6).
   - `summary` counts the rows by state, and counts the ADRs that are `undeclared`.
   - `--state proposed|accepted|completed|retired` (repeatable) filters. Retired rows are left out unless `--state retired` or `--all` is given.
   - `--adr <id>` reads one ADR.
   - A state that is not one of the four is refused with the code `bad-state`, and an ADR that is not in the ledger with `adr-not-found`; each error carries a `hint` and a `field`.
   - Rows are ordered by state (`completed`, `accepted`, `proposed`, `retired`, `unknown`), then by feature name without regard to case, then by ADR id.

6. **`released` is the first release that holds the feature.** For a `completed` row, it is the lowest version tag (`vX.Y.Z`, compared as numbers) that contains every commit named on the `**Built:**` lines that built it. It is `null` when a `**Built:**` line names no commit, when a named commit is not in the repository, or when no version tag contains them: the feature is built and not yet released. A row that is not `completed` has `released: null`.

7. **`hexa adr features --md --check <file>` fails when the file is not what `--md` would print now.** It exits 1 with the code `fact-sheet-stale`, naming the file and the command that rewrites it (`hexa adr features --md > <file>`). A project gates its committed fact sheet with it. A file that does not exist is not a pass: `fact-sheet-missing`. The Markdown carries no date and no commit of its own, so it changes only when a feature does.

8. **What the section says and the ADR cannot support is reported, not guessed.** Each is a warning on the row and in `warnings`; `--strict` makes any of them exit 1.
   - `unknown-phase`: a row names a phase the ADR does not have. The row's state is `unknown`.
   - `no-feature-column`: the section has a table with no `Feature` column. The ADR is treated as `undeclared`.
   - `empty-feature`: a row whose `Feature` cell is empty. The row is skipped.
   - `duplicate-feature`: two rows of one ADR with the same name, ignoring case. Both are kept.

9. **`hexa adr features extract <ADR>` drafts the `## Features` section for an ADR that has none.** It reads the ADR and asks the T1 tier for the features the decision gives a user, each tied to a phase when the ADR has phases. It prints the section and writes nothing; `--write` inserts it before `## Consequences` (or at the end when there is none). It refuses an ADR that already has the section (`features-declared`). The table it writes is a draft a person reads: the state of each row still comes from rule 2, so a wrong extraction can name a feature badly but cannot call it built.

## Phases

### F1. The domain rules and the table

Decisions 1 to 5, 7 and 8: `adr_features.rs`, the port, `hexa adr features` with `--json`, `--md`, `--check`, `--state`, `--adr`, `--all`, `--strict`.

### F2. The release a feature shipped in

Decision 6.

### F3. Extraction

Decision 9.

## Features

| Feature | What it does | Phase |
|---|---|---|
| Feature table | Lists every feature the ADRs name with its state: proposed, accepted, completed or retired | F1 |
| Fact sheet check | Fails a build when the committed feature fact sheet is out of date | F1 |
| Released-in column | Says which release first held each completed feature | F2 |
| Feature extraction | Drafts an ADR's feature table from its text for a person to review | F3 |

## Consequences

- A project's feature list is as true as its `**Built:**` lines. On datum today the board reads 5 of 176 ADRs as built, so the first table will show most features as `accepted`: the table makes missing evidence visible, and filling it in is work on the ADRs, not on this verb.
- A product owner reads one table to see what is proposed, decided and shipped, and in which release.
- An ADR template gains a section. `hexa adr schema` and the `hexa-adr-create` skill name it.

## Not decided / not built here

- A feature decided across several ADRs appears once per ADR. Joining them under one name is not decided.
- Tying a marketing claim to a feature row.
- A feature's later states (deprecated, removed in a release).

## Gate

`hexa spec verify ADR-2611250845`.
