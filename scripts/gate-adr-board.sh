#!/usr/bin/env bash
# Gate for ADR-2611151030: `hexa adr board` shows which decisions are done and who is on them.
# CI/gate tooling only; the behaviour it checks lives in the hexa binary.
#
# Usage: scripts/gate-adr-board.sh [--phase N]     (N = 1, 2 or 3; default 3 = all)
# Env:   HEXA_BIN  the binary under test. Default: built from this tree.
#                  The hexa on PATH is never used: a stale binary must not pass.
# Exit:  0 only when every selected check passed and at least one check ran.
#
# The expected numbers below are worked out by hand from the fixture, not read
# from the implementation (see the table in the comment above `mkadr`).
set -u

PHASE=3
if [ "${1:-}" = "--phase" ]; then PHASE="${2:-}"; fi
case "$PHASE" in 1|2|3) ;; *) echo "usage: $0 [--phase 1|2|3]" >&2; exit 2;; esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# CI builds this tree's binary at target/ci/hexa before it runs the ADR gates; use it rather than
# build a second copy. It never exists in a local checkout unless CI put it there.
if [ -z "${HEXA_BIN:-}" ] && [ -x "$ROOT/target/ci/hexa" ]; then HEXA_BIN="$ROOT/target/ci/hexa"; fi
if [ -z "${HEXA_BIN:-}" ]; then
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/gate-adr-board}"  # under target/: hexa analyze skips a directory of that name
  (cd "$ROOT" && cargo build -q -p hexa-cli) || { echo "FAIL build: cargo build -p hexa-cli failed"; exit 1; }
  HEXA_BIN="$CARGO_TARGET_DIR/debug/hexa"
fi
[ -x "$HEXA_BIN" ] || { echo "FAIL setup: no executable hexa at $HEXA_BIN"; exit 1; }
command -v jq >/dev/null || { echo "FAIL setup: jq is required"; exit 1; }

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
export HOME="$TMP/home"; mkdir -p "$HOME"; unset HEXA_HOME HEXA_TASK
REPO="$TMP/repo"; mkdir -p "$REPO"; cd "$REPO" || exit 1
git init -q -b main . && git config user.email g@example.com && git config user.name gate
mkdir -p docs/adrs .hexa

# mkadr <number> <status line> <title> <body>   writes docs/adrs/ADR-<number>-x.md
mkadr() { # $1 dir $2 number $3 status $4 title $5 body
  printf '# ADR-%s: %s\n\n**Status:** %s\n**Date:** 2026-11-14\n%s\n' "$2" "$4" "$3" "$5" > "$1/docs/adrs/ADR-$2-x.md"; }

# The fixture, and what the board must say about it (units are phases, or the ADR when it has none):
#   0001 Proposed, phases R1 R2 R3; R1 and R2 have a **Built line in their section;
#        a "### Decision gate (R1)" heading that is not a phase     -> building 2/3, drift status-behind-evidence
#   0002 Accepted, **Built:** header line, no phases                -> built 1/1
#   0003 Proposed, no phases, title holds <script>                  -> proposed 0/1
#   0004 Accepted, **Built:** P1 and P2a, phases P1 P2 P2a (no section markers)
#                                                                   -> building 2/3  (P2 is not named by "P2a")
#   0005 Superseded                                                 -> retired, outside the summary
#   0006 no status line at all                                      -> unknown, outside the summary
# Summary: units 3+1+1+3 = 8, built 2+1+0+2 = 5, percent floor(500/8) = 62.
mkadr . 2611150001 Proposed "Phased work" '
## Phases

### R1. First
**Built 2026-11-14.** It works.

### Decision gate (R1)
```sh
true
```

### R2. Second
**Built 2026-11-14.**

### R3. Third
Not yet.

## Gate

`true`
'
mkadr . 2611150002 Accepted "Whole thing" '**Built:** all of it on 2026-11-14.
'
mkadr . 2611150003 Proposed "<script>alert(1)</script> Title" '
Nothing here.
'
mkadr . 2611150004 Accepted "Headline built" '**Built:** P1 and P2a on 2026-11-14.

## Phases

### P1. One
Text.

### P2. Two
Text.

### P2a. Two a
Text.
'
mkadr . 2611150005 "Superseded by ADR-2611150002" "Old" ''
printf '# ADR-2611150006: No status\n\nText.\n' > docs/adrs/ADR-2611150006-x.md
git add -A && git commit -q -m fixture

# A second product: its own repository, its own ADRs. Built 1 of 2 units = 50%.
PROD="$TMP/product"; mkdir -p "$PROD/docs/adrs"; ( cd "$PROD" && git init -q -b main . && git config user.email g@example.com && git config user.name gate )
mkadr "$PROD" 2611150071 Accepted "Product thing one" '**Built:** done.
'
mkadr "$PROD" 2611150072 Proposed "Product thing two" ''
( cd "$PROD" && git add -A && git commit -q -m p )

RAN=0; FAILED=0
pass() { RAN=$((RAN+1)); echo "PASS  $1"; }
fail() { RAN=$((RAN+1)); FAILED=$((FAILED+1)); echo "FAIL  $1: $2"; }
# check <name> <phase> <function>: run only when the phase is selected.
check() { [ "$2" -le "$PHASE" ] || return 0; if "$3"; then pass "$1"; else fail "$1" "${WHY:-condition not met}"; fi; WHY=; }
H() { "$HEXA_BIN" "$@"; }
row() { jq -c --arg id "ADR-$1" '.adrs[] | select(.id==$id)' <<<"$OUT"; }

# ---- phase 1: the rules, one checkout ----------------------------------------
c1() { OUT="$(H adr board --json 2>&1)" || { WHY="board --json exited non-zero: $(head -c 200 <<<"$OUT")"; return 1; }
  jq -e '.ok==true and (.adrs|length)==6' >/dev/null <<<"$OUT" || { WHY="expected ok and 6 ADRs; got: $(head -c 200 <<<"$OUT")"; return 1; }; }
c2() { local r
  r="$(row 2611150001 | jq -c '[.state,.units.done,.units.total]')"; [ "$r" = '["building",2,3]' ] || { WHY="0001 (decision-gate heading must not count): $r"; return 1; }
  r="$(row 2611150002 | jq -c '[.state,.units.done,.units.total]')"; [ "$r" = '["built",1,1]' ] || { WHY="0002: $r"; return 1; }
  r="$(row 2611150003 | jq -c '[.state,.units.done,.units.total]')"; [ "$r" = '["proposed",0,1]' ] || { WHY="0003: $r"; return 1; }; }
c3() { local r
  r="$(row 2611150004 | jq -c '[.state,.units.done,.units.total,([.units.phases[]|select(.built)|.id])]')"
  [ "$r" = '["building",2,3,["P1","P2a"]]' ] || { WHY="0004 header ids as whole tokens, P2 is not P2a: $r"; return 1; }; }
c4() { local r
  r="$(row 2611150005 | jq -r .state)"; [ "$r" = retired ] || { WHY="0005 superseded should be retired: $r"; return 1; }
  r="$(row 2611150006 | jq -r .state)"; [ "$r" = unknown ] || { WHY="0006 with no status should be unknown: $r"; return 1; }; }
c5() { local r
  r="$(jq -c '[.summary.units,.summary.built,.summary.percent]' <<<"$OUT")"
  [ "$r" = '[8,5,62]' ] || { WHY="summary units/built/percent should be [8,5,62] (retired and unknown excluded, rounded down): $r"; return 1; }
  r="$(jq -c '.summary.by_state' <<<"$OUT")"
  jq -e '.built==1 and .building==2 and .proposed==1 and .retired==1 and .unknown==1 and (.accepted//0)==0' >/dev/null <<<"$r" || { WHY="by_state: $r"; return 1; }; }
c6() { local r
  r="$(row 2611150001 | jq -c '.drift')"; [ "$r" = '["status-behind-evidence"]' ] || { WHY="0001 is Proposed with built phases: drift $r"; return 1; }
  r="$(row 2611150002 | jq -c '.drift')"; [ "$r" = '[]' ] || { WHY="0002 has no drift: $r"; return 1; }
  r="$(row 2611150001 | jq -r .gate)"; [ "$r" = recorded ] || { WHY="0001 records a gate: $r"; return 1; }
  r="$(row 2611150002 | jq -r .gate)"; [ "$r" = none ] || { WHY="0002 records none: $r"; return 1; }; }
c7() { H adr board --json --strict >/dev/null 2>&1; [ $? -eq 1 ] || { WHY="--strict must exit 1 when an ADR has drift"; return 1; }
  H adr board --json >/dev/null 2>&1 || { WHY="without --strict drift is not a failure"; return 1; }; }
c8() { local o
  o="$(H adr board --text 2>&1)" || { WHY="--text exited non-zero"; return 1; }
  grep -q '5 of 8' <<<"$o" && grep -q 'ADR-2611150001' <<<"$o" && grep -q '62%' <<<"$o" \
    || { WHY="text board must name 5 of 8, 62% and the ADR ids; got: $(head -c 300 <<<"$o")"; return 1; }; }
# honest failure: no ADRs is an error, never a board of zero
c9() { local d o rc; d="$TMP/empty"; mkdir -p "$d/docs/adrs"; ( cd "$d" && git init -q . )
  o="$(cd "$d" && H adr board --json 2>&1)"; rc=$?
  [ $rc -eq 1 ] && jq -e '.ok==false and .code=="no_adrs"' >/dev/null <<<"$o" || { WHY="empty docs/adrs: exit $rc, $(head -c 160 <<<"$o")"; return 1; }
  d="$TMP/none"; mkdir -p "$d"; ( cd "$d" && git init -q . )
  o="$(cd "$d" && H adr board --json 2>&1)"; rc=$?
  [ $rc -eq 1 ] && jq -e '.ok==false and .code=="no_adrs"' >/dev/null <<<"$o" || { WHY="no docs/adrs: exit $rc, $(head -c 160 <<<"$o")"; return 1; }; }

# ---- phase 2: other checkouts and the ledger ---------------------------------
WT="$TMP/wt"
mkwt() { [ -d "$WT" ] && return 0
  git worktree add -q "$WT" -b other >/dev/null 2>&1 || return 1
  mkadr "$WT" 2611150003 Accepted "<script>alert(1)</script> Title" '**Built:** done in the other worktree.
'                                                                    # uncommitted would also do; this one is
  mkadr "$WT" 2611150099 Proposed "Only on a branch" ''
  printf '\nAmended in the branch, not committed.\n' >> "$WT/docs/adrs/ADR-2611150001-x.md"
  return 0; }
c10() { mkwt || { WHY="could not create the second worktree"; return 1; }
  OUT="$(H adr board --json 2>&1)" || { WHY="board exited non-zero with a worktree: $(head -c 200 <<<"$OUT")"; return 1; }
  local r; r="$(row 2611150003 | jq -c '[.elsewhere[0].branch,.elsewhere[0].status,.elsewhere[0].state,.elsewhere[0].differs]')"
  [ "$r" = '["other","accepted","built",true]' ] || { WHY="0003 changed in worktree 'other': $r"; return 1; }
  r="$(row 2611150002 | jq -c '.elsewhere')"; [ "$r" = '[]' ] || { WHY="0002 is identical in the worktree, so no entry: $r"; return 1; }; }
c11() { local r; r="$(row 2611150099 | jq -c '[.in_current,.elsewhere[0].branch]')"
  [ "$r" = '[false,"other"]' ] || { WHY="an ADR only on a branch is listed with in_current false: $r"; return 1; }
  r="$(jq -c '[.summary.units,.summary.built,.summary.percent]' <<<"$OUT")"
  [ "$r" = '[8,5,62]' ] || { WHY="another checkout must not change the summary: $r"; return 1; }; }
c12() { local d="$TMP/other-repo" o r; mkdir -p "$d/docs/adrs"; ( cd "$d" && git init -q . )
  mkadr "$d" 2611150003 Accepted "Same number elsewhere" ''
  o="$(H adr board --json --also "$d" 2>&1)" || { WHY="--also exited non-zero: $(head -c 200 <<<"$o")"; return 1; }
  r="$(jq -c --arg d "$d" '.adrs[]|select(.id=="ADR-2611150003")|[.elsewhere[]|select(.worktree==$d)|.status]' <<<"$o")"
  [ "$r" = '["accepted"]' ] || { WHY="--also checkout should appear under elsewhere: $r"; return 1; }
  o="$(H adr board --json --also "$TMP/does-not-exist" 2>&1)" || { WHY="a missing --also dir must be a warning, not a crash"; return 1; }
  jq -e '(.warnings|length)>=1 and .ok==true' >/dev/null <<<"$o" || { WHY="a missing --also dir needs a warning: $(head -c 200 <<<"$o")"; return 1; }; }
c13() { H agent register --task "implement ADR-2611150003" --lease src/c --owner alice --json >/dev/null 2>&1 \
    || { WHY="could not register a ledger task"; return 1; }
  OUT="$(H adr board --json 2>&1)"
  local r; r="$(row 2611150003 | jq -c '[.claimed_by[0].owner,.claimed_by[0].status]')"
  [ "$r" = '["alice","claimed"]' ] || { WHY="0003 is claimed by alice: $r"; return 1; }
  r="$(row 2611150002 | jq -c '.claimed_by')"; [ "$r" = '[]' ] || { WHY="0002 is claimed by nobody: $r"; return 1; }; }

# a difference has a kind, and whether it is uncommitted: only uncommitted work collides
c15() { local r
  r="$(row 2611150003 | jq -c '[.elsewhere[0].kind,.elsewhere[0].uncommitted]')"
  [ "$r" = '["status",true]' ] || { WHY="0003: status differs, file uncommitted: $r"; return 1; }
  r="$(row 2611150001 | jq -c '[.elsewhere[0].kind,.elsewhere[0].uncommitted]')"
  [ "$r" = '["text",true]' ] || { WHY="0001: only the text differs, file uncommitted: $r"; return 1; }; }
c16() { local wt2="$TMP/wt2" o r
  git worktree add -q "$wt2" -b behind >/dev/null 2>&1 || { WHY="could not create worktree 'behind'"; return 1; }
  printf '\nA committed edit on another branch.\n' >> "$wt2/docs/adrs/ADR-2611150002-x.md"
  ( cd "$wt2" && git add -A && git commit -q -m edit ) || { WHY="commit in 'behind' failed"; return 1; }
  OUT="$(H adr board --json 2>&1)"
  r="$(row 2611150002 | jq -c '[.elsewhere[]|select(.branch=="behind")|[.kind,.uncommitted]][0]')"
  [ "$r" = '["text",false]' ] || { WHY="0002: committed text-only edit elsewhere is kind text, uncommitted false: $r"; return 1; }
  o="$(H adr board --text 2>&1)"
  grep 'ADR-2611150002' <<<"$o" | grep -q 'behind' && { WHY="a committed text-only difference must not be a note in the table"; return 1; }
  grep 'ADR-2611150001' <<<"$o" | grep -q 'other' || { WHY="an uncommitted edit on branch 'other' must be a note on ADR 0001"; return 1; }
  return 0; }
# another product is its own board
c17() { local o r
  o="$(H adr board --json --product "$PROD" 2>&1)" || { WHY="--product exited non-zero: $(head -c 200 <<<"$o")"; return 1; }
  r="$(jq -c '[.products[0].summary.units,.products[0].summary.built,.products[0].summary.percent,(.products[0].adrs|length)]' <<<"$o")"
  [ "$r" = '[2,1,50,2]' ] || { WHY="product summary should be [2,1,50,2]: $r"; return 1; }
  r="$(jq -c '[.summary.units,.summary.built,.summary.percent,([.adrs[].id]|index("ADR-2611150071"))]' <<<"$o")"
  [ "$r" = '[8,5,62,null]' ] || { WHY="a product must not change this repository's summary or rows: $r"; return 1; }
  o="$(H adr board --json --also "$PROD" 2>&1)" || { WHY="--also with a foreign repo crashed"; return 1; }
  jq -e '[.warnings[]|select(contains("--product"))]|length>=1' >/dev/null <<<"$o" || { WHY="--also of an unrelated repo must warn and name --product: $(jq -c .warnings <<<"$o")"; return 1; }
  o="$(H adr board --json --product "$TMP/does-not-exist" 2>&1)" || { WHY="a missing --product dir must be a warning or error object, not a crash"; return 1; }
  return 0; }

# ---- phase 3: the page -------------------------------------------------------
c14() { local f="$TMP/board.html"
  H adr board --html "$f" --product "$PROD" >/dev/null 2>&1 || { WHY="--html exited non-zero"; return 1; }
  [ -s "$f" ] || { WHY="--html wrote nothing"; return 1; }
  grep -q 'ADR-2611150001' "$f" && grep -q '62%' "$f" || { WHY="page must name the ADRs and 62%"; return 1; }
  grep -q 'ADR-2611150071' "$f" && grep -q '50%' "$f" || { WHY="page must show the product's ADR and its own 50%"; return 1; }
  grep -qi '<script' "$f" && { WHY="page contains a <script tag (an ADR title must be escaped, and the page runs no script)"; return 1; }
  grep -q '&lt;script&gt;' "$f" || { WHY="ADR title was not escaped"; return 1; }
  grep -qiE 'https?://|src=|@import' "$f" && { WHY="page reaches outside itself"; return 1; }
  return 0; }

check "1  one checkout: ok, six ADRs"                                   1 c1
check "2  units: phases, decision-gate heading is not one, whole ADR"    1 c2
check "3  header **Built:** names phases as whole tokens"                1 c3
check "4  superseded is retired, no status is unknown"                   1 c4
check "5  summary 5 of 8 units, 62%, rounded down, by_state"             1 c5
check "6  drift and gate presence"                                       1 c6
check "7  --strict exits 1 on drift"                                     1 c7
check "8  --text names 5 of 8, 62% and the ADRs"                         1 c8
check "9  no ADRs is an error, never a board of zero"                    1 c9
check "10 a worktree that changed an ADR is listed under elsewhere"      2 c10
check "11 an ADR only on a branch; the summary ignores other checkouts"  2 c11
check "12 --also DIR; a missing one is a warning"                        2 c12
check "13 a ledger task is joined to its ADR"                            2 c13
check "14 a difference has a kind and is marked uncommitted or not"     2 c15
check "15 a committed text-only difference is not a table note"          2 c16
check "16 --product is its own board; --also of a foreign repo warns"    2 c17
check "17 --html: self-contained, inert, escaped, with each product"  3 c14

echo "---"
[ "$RAN" -gt 0 ] || { echo "FAIL: no check ran"; exit 1; }
echo "$((RAN-FAILED)) of $RAN checks pass"
# The summary in the form `hexa adr gates` reads (TAP's `# tests`, `# pass`, `# fail`), so the reader
# counts the checks itself and fails a run that counted none, instead of this gate opting out with
# `## Gate (uncounted)`.
echo "# tests $RAN"; echo "# pass $((RAN-FAILED))"; echo "# fail $FAILED"
[ "$FAILED" -eq 0 ]
