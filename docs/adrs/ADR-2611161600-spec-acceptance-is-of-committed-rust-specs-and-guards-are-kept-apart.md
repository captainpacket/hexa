# ADR-2611161600: Spec acceptance is of committed Rust specs, and guard tests are kept apart

**Status:** Proposed
**Date:** 2026-11-16
**Drivers:** An audit of verbs that report success they did not earn (branch `fix/no-false-success`). Three of its fixes to `hexa spec` contradict accepted, sealed specs. An agent may not edit a sealed test, so these fixes change through this ADR, then the specs, then a person's acceptance.
**Supersedes:** none. Amends ADR-2610221540 (S15, and the fixtures of S24, S24b and S24d), ADR-2610281516 (fixture setup), ADR-2610281628 (S06, S07) and ADR-2611062141 (fixture setup).

## Context

The audit ran each case below and confirmed it:

- `hexa spec accept` accepts a spec file that is untracked or modified, so the acceptance binds a person to text that no commit holds.
- `hexa spec tests` generates Rust only, but it says so only after a paid `spec draft` and a person's `spec accept`.
- When some generated tests already pass before the code (guards of behaviour already in place), `hexa spec tests` throws the whole generation away, including the tests that do fail first.

The branch fixed everything else in the audit. These three conflict with sealed tests:

1. **Refusing an untracked or modified spec at accept** breaks:
   - The fixtures of `a_decision_is_covered_by_a_sealed_spec.rs` and `a_test_was_red_before_the_code.rs` (ADR-2610221540). Both write the spec and accept it without committing.
   - The `adr_2610281516_spec.rs` setup, which runs `draft` and then `accept`, and commits only after.
   - `adr_2610281628_spec.rs` S06 and S07, which re-accept an edited working-tree spec. That is by design: the predecessor acceptance is read from HEAD (§1).
2. **Refusing `spec accept` without a Cargo.toml** breaks the `adr_2611062141_spec.rs` fixtures. They accept specs in a repository that has no manifest.
3. **Keeping the red tests by default** breaks ADR-2610221540 S15's sealed test, `a_generation_with_a_test_green_before_the_code_is_rejected`. It asserts that the whole generation is refused and nothing is committed.

What shipped on the branch without touching a sealed test:

- `spec draft` refuses with no Cargo.toml. The check runs after the free completion-path check and before the paid call.
- `spec tests --keep-red` keeps the tests that fail first and reports the passing ones as guards. Their scenarios stay pending.
- A refusal for green tests records a hit for the `guard-test-is-a-mirror` gap.
- `spec verify` fails on 0 passed with any scenario pending.

## Decision

1. **A first acceptance is of a committed spec.** `hexa spec accept` refuses a spec that is untracked, or modified while HEAD holds no acceptance of it. A re-acceptance may still be of an edited working-tree spec: its predecessor is anchored in HEAD and only the changed scenarios lose their proof (ADR-2610281628 §1). The refusal names `git add` and `git commit` as the step to take.
2. **`spec accept` refuses a project with no Cargo.toml**, with the same message as `spec draft`, until tests are generated for another language.
3. **`--keep-red` becomes the default for red at HEAD.** A generated test that passes before the code is left out and reported as a guard, and its scenario stays pending. A generation in which every test passes is still refused. With `--red-at`, a green test still refuses the generation (ADR-2610281442 §3). S15's "then" reads: "the test is rejected as testing nothing, and the tests that fail are kept".
4. **The fixtures change with the specs.** The affected fixtures commit the spec before they accept it, and the ADR-2611062141 fixture gains a Cargo.toml. Each change is an amendment (`hexa spec amend`) or a re-acceptance by a person. No agent edits a sealed test.

## Consequences

- An acceptance always names text that a commit holds, so `spec verify --prove` and the seals can be traced to it.
- A non-Rust project learns that specs cannot be sealed before anyone spends money or attention on one.
- A spec with negative guard scenarios gets its red tests sealed in one pass instead of none. The guards stay visible as pending scenarios until a test also exercises the new behaviour, as the guard guide describes.

## Found alongside (not decided here)

- `hexa analyze`'s deep pass fails on the whole tree when one file is not valid UTF-8. The grade then falls back to the import scan's count, a weaker claim printed as the same letter. The branch labels it in `--json` as `score_source: "import-scan (…)"`. The pass should skip and report the unreadable file instead (ADR-2609211430 classifies an unreadable file as a warning).

## Implementation

**Gate:** `cargo test -p hexa-cli --test a_verb_reports_only_the_success_it_earned && cargo test -p hexa-exec --test keep_red_keeps_the_tests_that_fail_first`. These are the tests the fixes extend. The amended sealed tests join the gate when they are re-sealed.

## References

- Branch `fix/no-false-success`.
- ADR-2610221540, ADR-2610281442, ADR-2610281516, ADR-2610281628, ADR-2611062141, ADR-2609211430.
