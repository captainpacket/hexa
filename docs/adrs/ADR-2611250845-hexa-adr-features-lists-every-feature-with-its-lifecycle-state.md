# ADR-2611250845: `hexa adr features` lists every feature the decisions name, with where each stands

**Status:** Accepted
> 2026-11-25 Accepted: the owner asked for it (gary, 2026-11-25, in datum: "we need a definitive fact sheet on all features, so we can manage the product lifecycle", then "we should be able to extract a feature from an ADR than lists that feature in a table relevant to proposed, accepted or completed, this should be in hexa", then "start the hexa features verb now"). Recorded by the implementing agent on his behalf. The spec is drafted, not accepted: `hexa spec accept ADR-2611250845` is his.
> 2026-11-25 The spec was accepted by Gary Berger (`e985045`). Its 27 scenarios are 28 tests, written by hand from the scenarios, red at `25e9d2d` on tt (hexa-core 16 failed on the unimplemented panic, hexa-cli 12 failed on their assertions, none passed) and sealed at `0b3fceb`.
**Date:** 2026-11-25
**Built:** F1 on 2026-11-25, `e666a39` and `77e6a05`, on `feat/adr-features`: the domain rules, the port, and `hexa adr features` with `--json`, `--text`, `--md`, `--check`, `--state`, `--adr`, `--all` and `--strict`. On tt at `77e6a05`: S01 to S18 and S21 to S25 pass.
**Built:** F2 on 2026-11-25, `e666a39`: `released`, read through `hexa-git`. On tt at `77e6a05`: S19 and S20 pass.
**Built:** F3 on 2026-11-25, `e666a39`: `hexa adr features extract`, through the T1 tier. On tt at `77e6a05`: S26 and S27 pass. `hexa spec verify ADR-2611250845` there: 27 passed, 0 failed.
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

## Addendum: what the build settled (2026-11-25)

The decisions and the 27 scenarios left these open. Each was settled in the build, by the builder, and is the owner's to overrule.

- **A proposed ADR with part built.** A row is `proposed` only while nothing that builds it is built. With some of it built the row is `accepted`, as the board calls that ADR `building` (S12 says so for a row that names no phase; a row that names phases follows the same rule).
- **A retired or unreadable ADR decides the row first.** A row of a superseded ADR is `retired` and a row of an ADR whose status cannot be read is `unknown`, whatever its phases say.
- **`adr_title` is the title without the ADR's id**, as the feature name of an undeclared ADR is.
- **`summary`.** It counts every row read, listed or not, so `--state` does not change it. `by_state` names all five states, at zero too. `undeclared` counts the ADRs whose title stands in for a feature, so a retired ADR with no section is not counted. `summary.features` and `summary.adrs` are the totals.
- **`warnings`** is a list of `{code, adr, feature, message}`; `feature` is null for a finding about the section (`no-feature-column`, `empty-feature`). `empty-feature` is listed once for each row skipped.
- **Errors** are one object on stdout with `ok: false`, `code`, `message`, `hint`, `field` and `retryable`, as `hexa adr board` prints them. With `--md` and no `--check` the object goes to stderr, so a failed `--md > FEATURES.md` does not write an error into the sheet. A project with no `docs/adrs/` is refused with the board's code, `no_adrs`.
- **`--check` implies `--md`.** It compares bytes. The command it names repeats `--all`, `--state` and `--adr` when they were given. A file that cannot be read for any reason is `fact-sheet-missing`.
- **`--state` takes only the four states.** A row that is `unknown` is listed by default and by `--all`, and cannot be asked for alone. `--all` with `--state` lists everything.
- **The fact sheet.** A `# Features` heading, one sentence of counts (`5 features in 4 ADRs: 2 completed, 1 accepted, 1 proposed, 1 retired.`, then how many are listed when that is fewer), a sentence when a title stands in, then `## Completed`, `## Accepted`, `## Proposed`, `## Retired`, `## Unknown` for the states that have rows. A title standing in is marked `*(ADR title)*`. `Released` is the version, `not released` for a completed row without one, and empty otherwise.
- **Which `**Built` lines built a row** (decision 6). For a row that names phases: the header `**Built:**` lines that name one of them, and the `**Built` lines under those phases' headings. For a row that names none: every `**Built` line. A commit is a word of 7 to 40 lower-case hex digits that does not follow `ADR-`. A long number on such a line is therefore read as a commit the repository does not hold, which leaves `released` null and never names a wrong release. A row completed by a `completed` status with no `**Built` line has `released: null`.
- **Git is read through `hexa-git`** (`hexa_git::tags_containing`, in `hexa-git/src/release.rs`), once per commit. Outside a repository every `released` is null and nothing fails.
- **Extraction** (decision 9). The T1 model is `inference.tier_models.t1`; with none configured the verb refuses with `no-t1-model`. The model is asked for one Markdown table. The verb reads the first table in the reply that has a `Feature` column, keeps the name, the sentence and the phases, drops every other column and every phase the ADR does not have, and writes the three-column table itself. `extract` prints the section as Markdown, also when stdout is not a terminal, and `--json` prints it with its rows. A reply with no such table is `extract-unreadable`, a tier that does not answer is `extract-failed` (both `retryable`), and `--write` refuses a draft with no rows (`extract-empty`): a section with no rows is a statement a person makes. An ADR that has the heading already is refused, whatever is under it.
- **The tests.** `hexa spec tests` could not run (no completion model was reachable), so the tests were written by hand from the accepted scenarios, by the route `hexa gap pr link-hand-written-tests` prints. S15 has two tests: the spec links the command-line one, and its sort half is `s15_sort_features_orders_by_state_then_name_without_case_then_adr_id` in the sealed `hexa-core` file. The tests fix three things the scenarios did not: the T1 model is stubbed with `HEXA_COMPLETION_COMMAND`; the fact sheet states its counts as `<n> <state>`; and a title standing in carries the word `title` on its row.
- **Source was committed with `git commit`, not `hexa do gate`.** This repository declares no `test.remote`, so `do gate` would have built on the developer's Mac, which the owner forbids. Every build and test ran on `tt`, for the pushed commit.

## Gate

`hexa spec verify ADR-2611250845`.
