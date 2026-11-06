#!/usr/bin/env bash
# Gate for ADR-2611051850: hexa keeps the record and the rules for concurrent agents.
# CI/gate tooling only; the behaviour it checks lives in the hexa binary.
#
# Usage: scripts/gate-run-ledger.sh [--phase N]     (N = 1, 2 or 3; default 3 = all)
# Env:   HEXA_BIN  the binary under test. Default: built from this tree.
#                  The hexa on PATH is never used: a stale binary must not pass.
# Exit:  0 only when every selected check passed and at least one check ran.
set -u

PHASE=3
if [ "${1:-}" = "--phase" ]; then PHASE="${2:-}"; fi
case "$PHASE" in 1|2|3) ;; *) echo "usage: $0 [--phase 1|2|3]" >&2; exit 2;; esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [ -z "${HEXA_BIN:-}" ]; then
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target-wt}"
  (cd "$ROOT" && cargo build -q -p hexa-cli) || { echo "FAIL build: cargo build -p hexa-cli failed"; exit 1; }
  HEXA_BIN="$CARGO_TARGET_DIR/debug/hexa"
fi
[ -x "$HEXA_BIN" ] || { echo "FAIL setup: no executable hexa at $HEXA_BIN"; exit 1; }
command -v jq >/dev/null || { echo "FAIL setup: jq is required"; exit 1; }

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
export HOME="$TMP/home"; mkdir -p "$HOME"; unset HEXA_HOME
REPO="$TMP/repo"; mkdir -p "$REPO"; cd "$REPO" || exit 1
git init -q . && git config user.email g@example.com && git config user.name gate
mkdir -p src/a src/b tests docs/specs .hexa
echo 'fn a() {}' > src/a/x.rs; echo 'fn b() {}' > src/b/y.rs; echo 't' > tests/sealed_spec.rs
printf '{"sealed":{"tests/sealed_spec.rs":"00","tests/gone_spec.rs":"00"}}\n' > docs/specs/ADR-2611000000.json
git add -A && git commit -q -m init

RAN=0; FAILED=0
pass() { RAN=$((RAN+1)); echo "PASS  $1"; }
fail() { RAN=$((RAN+1)); FAILED=$((FAILED+1)); echo "FAIL  $1: $2"; }
# check <name> <phase> <function>: run only when the phase is selected.
check() { [ "$2" -le "$PHASE" ] || return 0; if "$3"; then pass "$1"; else fail "$1" "${WHY:-condition not met}"; fi; WHY=; }
H() { "$HEXA_BIN" "$@"; }

c1() { local o; o="$(H status --json 2>&1)" || { WHY="status --json exited non-zero: $(echo "$o" | head -1)"; return 1; }
  echo "$o" | jq -e '(.runs|type=="array") and (.tasks|type=="array") and .count.runs==0 and .count.tasks==0' >/dev/null \
    || { WHY="expected empty runs/tasks with count 0 and 0; got: $(echo "$o" | head -c 120)"; return 1; }; }
# 1b: a record that cannot be read is reported as unreadable, never as a count of zero.
c1b() { local o rc
  mkdir -p "$HOME/.hexa"; : > "$HOME/.hexa/runs"      # a file where the runs directory must be
  o="$(H status --json 2>&1)"; rc=$?; rm -f "$HOME/.hexa/runs"
  [ $rc -eq 0 ] || { WHY="status --json crashed on an unreadable runs dir (exit $rc): $(echo "$o" | head -1)"; return 1; }
  echo "$o" | jq -e '.count.runs==null and .ok==false and (.unreadable|length)>=1' >/dev/null \
    || { WHY="unreadable runs dir did not read as unreadable (count.runs must be null); got: $(echo "$o" | head -c 160)"; return 1; }; }
# 1c: a real run writes a record that status lists with its outcome, and pid_start is set
#     (macOS and Linux). `bench` over an empty corpus is a real run that needs no model.
c1c() { local o f; mkdir -p "$TMP/empty-corpus"
  H bench agentic --corpus "$TMP/empty-corpus" >/dev/null 2>&1 || { WHY="bench over an empty corpus failed"; return 1; }
  o="$(H status --json 2>&1)" || { WHY="status failed"; return 1; }
  echo "$o" | jq -e '.recent[0].verb=="bench" and .recent[0].outcome=="passed" and .count.runs==0' >/dev/null \
    || { WHY="finished bench run not listed as passed in recent: $(echo "$o" | head -c 200)"; return 1; }
  f="$HOME/.hexa/runs/$(echo "$o" | jq -r '.recent[0].id').json"
  jq -e '.pid_start!=null and .pid_start_unknown==false and .ended!=null and .outcome=="passed"' "$f" >/dev/null 2>&1 \
    || { WHY="run record $f lacks pid_start or ended"; return 1; }; }
c2() { T1="$(H agent register --task one --lease src/a/ --json 2>&1)" && T2="$(H agent register --task two --lease src/b/ --json 2>&1)" \
    || { WHY="register failed: $(echo "${T1:-}" | head -1)"; return 1; }
  ID1="$(echo "$T1" | jq -er .id)" && ID2="$(echo "$T2" | jq -er .id)" && [ "$ID1" != "$ID2" ] || { WHY="no distinct ids in output"; return 1; }; }
c3() { local o; o="$(H status --json 2>&1)" || { WHY="status failed"; return 1; }
  echo "$o" | jq -e --arg a "${ID1:-x}" --arg b "${ID2:-y}" '.count.tasks==2 and ([.tasks[].id]|index($a)!=null and index($b)!=null) and (.tasks|length)==2' >/dev/null \
    || { WHY="expected exactly tasks $ID1 and $ID2 with count 2; got: $(echo "$o" | head -c 120)"; return 1; }; }
c4() { local o rc; o="$(H agent register --task three --lease src/a/x.rs --json 2>&1)"; rc=$?
  [ $rc -ne 0 ] || { WHY="overlapping lease was accepted"; return 1; }
  echo "$o" | jq -e 'has("code") and has("hint") and has("field") and has("retryable")' >/dev/null \
    || { WHY="refusal lacks code/hint/field/retryable: $(echo "$o" | head -c 120)"; return 1; }; }
c5() { local p1 p2 rc err
  p1="{\"hook_event_name\":\"PreToolUse\",\"tool_name\":\"Edit\",\"tool_input\":{\"file_path\":\"$REPO/src/b/y.rs\"}}"
  p2="{\"hook_event_name\":\"PreToolUse\",\"tool_name\":\"Edit\",\"tool_input\":{\"file_path\":\"$REPO/src/a/x.rs\"}}"
  err="$(echo "$p1" | HEXA_TASK="${ID1:-none}" H hook pre-edit 2>&1 >/dev/null)"; rc=$?
  [ $rc -eq 2 ] || { WHY="edit outside the lease exited $rc, wanted 2"; return 1; }
  echo "$err" | grep -q "${ID1:-none}" || { WHY="refusal does not name the task: $(echo "$err" | head -c 100)"; return 1; }
  echo "$p2" | HEXA_TASK="${ID1:-none}" H hook pre-edit >/dev/null 2>&1; rc=$?
  [ $rc -eq 0 ] || { WHY="edit inside the lease exited $rc, wanted 0"; return 1; }; }
c6() { local o rc
  for pth in tests/sealed_spec.rs .runner; do
    o="$(H agent register --task nolease --lease "$pth" --json 2>&1)"; rc=$?
    [ $rc -ne 0 ] || { WHY="$pth was leasable"; return 1; }
    echo "$o" | jq -e '.code=="lease_not_allowed" and has("hint") and has("field") and .retryable==false' >/dev/null 2>&1 \
      || { WHY="$pth refused for the wrong reason (want code lease_not_allowed): $(echo "$o" | head -c 100)"; return 1; }
  done; }
c7() { local o; echo 'fn a() { 1 }' > src/a/x.rs; echo 'token' > src/a/.runner
  o="$(H do run "edit" --file src/a/x.rs --evidence true --plan-only --json 2>&1)" || { WHY="plan-only failed: $(echo "$o" | head -1)"; return 1; }
  echo "$o" | jq -e '(.would_commit|index("src/a/x.rs")!=null) and (.would_commit|index("src/a/.runner")==null) and (.left_uncommitted|index("src/a/.runner")!=null)' >/dev/null \
    || { WHY="expected x.rs committed and .runner left; got: $(echo "$o" | head -c 160)"; return 1; }; }
c8() { local o; o="$(H do run "edit" --file src/a/x.rs --evidence true --plan-only --json 2>&1)" || { WHY="refused: $(echo "$o" | head -1)"; return 1; }
  echo "$o" | jq -e '(.refused // false)==false' >/dev/null || { WHY="a sealed file absent before and after read as an edit"; return 1; }; }
c9() { local o f
  o="$(H agent report --id "${ID1:-none}" --outcome passed --commit abc1234 --json 2>&1)" || { WHY="report failed: $(echo "$o" | head -1)"; return 1; }
  o="$(H status --json 2>&1)"
  echo "$o" | jq -e --arg a "${ID1:-x}" '(.tasks[]|select(.id==$a)|.status=="done") and .count.tasks==1' >/dev/null \
    || { WHY="task one not done or running-task count not 1: $(echo "$o" | head -c 120)"; return 1; }
  f="$HOME/.hexa/runs/$(echo "$o" | jq -r --arg a "${ID1:-x}" '.tasks[]|select(.id==$a)|.run')".json
  jq -e '.outcome=="passed" and .commit=="abc1234" and (.pid_start!=null) and (.ended!=null)' "$f" >/dev/null 2>&1 \
    || { WHY="run record $f lacks outcome/commit/pid_start/ended"; return 1; }; }

check "1 empty state reports zero runs and zero tasks"            1 c1
check "1b an unreadable runs dir is reported, not counted as zero" 1 c1b
check "1c a real run leaves a record with outcome and pid_start"   1 c1c
check "2 two tasks with disjoint leases register"                 2 c2
check "3 status --json lists both, count exactly 2"               2 c3
check "4 overlapping lease refused with code/hint/field/retryable" 2 c4
check "5 edit outside the lease refused, inside allowed"          2 c5
check "6 sealed path and .runner are not leasable"                2 c6
check "7 do leaves an untracked .runner uncommitted"              2 c7
check "8 a deleted sealed file does not read as an edit"          2 c8
check "9 report closes task and run record (pid_start set)"       3 c9

echo "---- $RAN checks, $FAILED failed (phase $PHASE, HEXA_BIN=$HEXA_BIN)"
[ "$RAN" -gt 0 ] || { echo "FAIL vacuous: no check ran"; exit 1; }
[ "$FAILED" -eq 0 ]
