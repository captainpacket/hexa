#!/usr/bin/env bash
# Gate for ADR-2611151330: `hexa dev build-keys` catches a native build that recompiles per command.
# CI/gate tooling only; the behaviour it checks lives in the hexa binary.
#
# Usage: scripts/gate-build-keys.sh [--phase N]     (N = 1 or 2; default 2 = all)
# Env:   HEXA_BIN  the binary under test. Default: built from this tree.
#                  The hexa on PATH is never used: a stale binary must not pass.
# Exit:  0 only when every selected check passed and at least one check ran.
#
# Needs: cargo with `cc` in its offline registry cache, and jq. No compiler runs: the
# fixtures are only ever asked for their unit graph.
#
# The fixtures reproduce the two mechanisms found in Strata (ADR-2611151330, Context):
#   profile  `shared` is a build dependency of the native script AND a normal dependency of `app`.
#            `-p native-sys` builds it host-only (debuginfo 0); `-p app` builds it once for the
#            target (debuginfo 2), and the script's tree differs.
#   features `app` has a build dependency on `shared` with feature `extra`; `native-sys` does not.
#            Build-dependency features unify per command over the selected packages.
set -u

PHASE=2
if [ "${1:-}" = "--phase" ]; then PHASE="${2:-}"; fi
case "$PHASE" in 1|2) ;; *) echo "usage: $0 [--phase 1|2]" >&2; exit 2;; esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# CI builds this tree's binary at target/ci/hexa before it runs the ADR gates; use it rather than
# build a second copy. It never exists in a local checkout unless CI put it there.
if [ -z "${HEXA_BIN:-}" ] && [ -x "$ROOT/target/ci/hexa" ]; then HEXA_BIN="$ROOT/target/ci/hexa"; fi
if [ -z "${HEXA_BIN:-}" ]; then
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/gate-build-keys}"  # under target/: hexa analyze skips a directory of that name
  (cd "$ROOT" && cargo build -q -p hexa-cli) || { echo "FAIL build: cargo build -p hexa-cli failed"; exit 1; }
  HEXA_BIN="$CARGO_TARGET_DIR/debug/hexa"
  unset CARGO_TARGET_DIR   # the fixtures must not share the hexa build's target directory
fi
[ -x "$HEXA_BIN" ] || { echo "FAIL setup: no executable hexa at $HEXA_BIN"; exit 1; }
command -v jq >/dev/null || { echo "FAIL setup: jq is required"; exit 1; }

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
export HEXA_HOME="$TMP/hexa-home"; mkdir -p "$HEXA_HOME"; unset HEXA_TASK CARGO_TARGET_DIR

# mkws <dir> <variant>: profile | profile-fixed | features | nocc
mkws() {
  local d="$1" v="$2"
  mkdir -p "$d"/{shared/src,native-sys/src,app/src}
  printf '[workspace]\nresolver = "3"\nmembers = ["shared", "native-sys", "app"]\n' > "$d/Cargo.toml"
  [ "$v" = profile-fixed ] && printf '\n[profile.dev.build-override]\ndebug = true\n' >> "$d/Cargo.toml"
  printf '[package]\nname = "shared"\nversion = "0.1.0"\nedition = "2021"\n[features]\nextra = []\n' > "$d/shared/Cargo.toml"
  : > "$d/shared/src/lib.rs"
  local cc='cc = "1"'; [ "$v" = nocc ] && cc=''
  printf '[package]\nname = "native-sys"\nversion = "0.1.0"\nedition = "2021"\nbuild = "build.rs"\n[build-dependencies]\n%s\nshared = { path = "../shared" }\n' "$cc" > "$d/native-sys/Cargo.toml"
  echo 'fn main() {}' > "$d/native-sys/build.rs"; : > "$d/native-sys/src/lib.rs"
  case "$v" in
    features)
      printf '[package]\nname = "app"\nversion = "0.1.0"\nedition = "2021"\nbuild = "build.rs"\n[dependencies]\nnative-sys = { path = "../native-sys" }\n[build-dependencies]\nshared = { path = "../shared", features = ["extra"] }\n' > "$d/app/Cargo.toml"
      echo 'fn main() {}' > "$d/app/build.rs" ;;
    *)
      printf '[package]\nname = "app"\nversion = "0.1.0"\nedition = "2021"\n[dependencies]\nnative-sys = { path = "../native-sys" }\nshared = { path = "../shared" }\n' > "$d/app/Cargo.toml" ;;
  esac
  : > "$d/app/src/lib.rs"
  (cd "$d" && cargo generate-lockfile --offline -q 2>&1) || return 1
}
for v in profile profile-fixed features nocc; do
  mkws "$TMP/$v" "$v" || { echo "FAIL setup: could not build the '$v' fixture offline (is cc in the cargo registry cache?)"; exit 1; }
done

RAN=0; FAILED=0
pass() { RAN=$((RAN+1)); echo "PASS  $1"; }
fail() { RAN=$((RAN+1)); FAILED=$((FAILED+1)); echo "FAIL  $1: $2"; }
check() { [ "$2" -le "$PHASE" ] || return 0; if "$3"; then pass "$1"; else fail "$1" "${WHY:-condition not met}"; fi; WHY=; }
# bk <fixture> [args]: run in the fixture; OUT is stdout, RC the exit code.
bk() { local d="$1"; shift; OUT="$(cd "$d" && "$HEXA_BIN" dev build-keys "$@" 2>/dev/null)"; RC=$?; }

# ---- phase 1: the rules and the verb -----------------------------------------
c1() { bk "$TMP/profile" --json
  [ "$RC" -eq 1 ] || { WHY="an unstable native script must exit 1, got $RC: $(head -c 200 <<<"$OUT")"; return 1; }
  jq -e '.ok==true and .stable==false and .summary.unstable==1 and .summary.native_scripts==1' >/dev/null <<<"$OUT" \
    || { WHY="summary: $(jq -c .summary <<<"$OUT" 2>&1 | head -c 200)"; return 1; }; }
c2() { bk "$TMP/profile" --json
  jq -e '.summary.selections==9' >/dev/null <<<"$OUT" || { WHY="3 members -> 3 workspace + 6 per-member selections = 9, got $(jq -c .summary.selections <<<"$OUT")"; return 1; }; }
# The groups below are cargo's own answer, read from its raw `--unit-graph` for the profile fixture:
# `shared` in the script's tree has debuginfo 0 under `build -p native-sys`, `test --no-run -p native-sys` and
# `clippy --workspace --all-targets` (host-only), and debuginfo 2 under `build -p app`, `test --no-run -p app`,
# `build --workspace` and `test --no-run --workspace` (shared with the target).
c3() { bk "$TMP/profile" --json; local r
  r="$(jq -c '.scripts[0] | [(.package|startswith("native-sys")), .stable, (.groups|length)]' <<<"$OUT")"
  [ "$r" = '[true,false,2]' ] || { WHY="the script is native-sys, unstable, in 2 groups of selections: $r"; return 1; }
  jq -e '(.scripts[0].groups | map(select(index("build -p native-sys")))[0]) as $host
         | (.scripts[0].groups | map(select(index("build -p native-sys") | not))[0]) as $target
         | ($host | index("test --no-run -p native-sys") and index("clippy --workspace --all-targets"))
           and ($target | index("build -p app") and index("test --no-run -p app") and index("build --workspace") and index("test --no-run --workspace"))' >/dev/null <<<"$OUT" \
    || { WHY="host-only group and target-shared group as cargo's graph says: $(jq -c .scripts[0].groups <<<"$OUT" | head -c 400)"; return 1; }; }
c4() { bk "$TMP/profile" --json; local r
  r="$(jq -c '[.scripts[0].differences[] | select(.unit|startswith("shared")) | .kind] | unique' <<<"$OUT")"
  [ "$r" = '["profile"]' ] || { WHY="shared differs by kind profile: $r"; return 1; }
  jq -e '.scripts[0].hint | contains("build-override") and contains("prediction")' >/dev/null <<<"$OUT" || { WHY="profile hint names build-override and says it is a prediction"; return 1; }; }
c5() { bk "$TMP/features" --json; local r
  [ "$RC" -eq 1 ] || { WHY="unstable by features must exit 1, got $RC"; return 1; }
  r="$(jq -c '[.scripts[0].differences[] | select(.unit|startswith("shared")) | .kind] | unique' <<<"$OUT")"
  [ "$r" = '["features"]' ] || { WHY="shared differs by kind features: $r"; return 1; }
  jq -e '.scripts[0].hint | contains("workspace-hack") and contains("build-dependency")' >/dev/null <<<"$OUT" || { WHY="features hint names a workspace-hack build-dependency"; return 1; }; }
c6() { bk "$TMP/profile-fixed" --json
  [ "$RC" -eq 0 ] || { WHY="the build-override fix must make it stable (exit 0), got $RC: $(jq -c '.scripts[0].differences' <<<"$OUT" | head -c 300)"; return 1; }
  jq -e '.stable==true and .summary.unstable==0 and .summary.native_scripts==1' >/dev/null <<<"$OUT" || { WHY="stable, 1 native script checked: $(jq -c .summary <<<"$OUT")"; return 1; }; }
# dev and test are different profile NAMES with equal settings: not a difference (cargo shares their artifacts)
c7() { bk "$TMP/profile-fixed" --json --selection "build --workspace" --selection "test --no-run --workspace"
  [ "$RC" -eq 0 ] && jq -e '.summary.selections==2 and .stable==true' >/dev/null <<<"$OUT" || { WHY="dev vs test with equal settings must be one key: rc $RC $(jq -c .summary <<<"$OUT")"; return 1; }; }
c8() { bk "$TMP/nocc" --json
  [ "$RC" -eq 0 ] && jq -e '.summary.native_scripts==0 and (.notes|map(test("no native build script"))|any)' >/dev/null <<<"$OUT" \
    || { WHY="no cc anywhere: exit 0, native_scripts 0, and it says so: rc $RC $(head -c 240 <<<"$OUT")"; return 1; }; }
# honest failure: it never passes because it could not look
c9() { bk "$TMP/profile" --json --selection "build -p native-sys"
  [ "$RC" -eq 2 ] && jq -e '.ok==false and .code=="too_few_selections"' >/dev/null <<<"$OUT" || { WHY="one selection compares nothing: exit 2 too_few_selections, got $RC $(head -c 200 <<<"$OUT")"; return 1; }; }
c10() { mkdir -p "$TMP/broken"; printf '[workspace\nmembers = [\n' > "$TMP/broken/Cargo.toml"
  bk "$TMP/broken" --json
  [ "$RC" -eq 2 ] && jq -e '.ok==false and .code=="no_unit_graph"' >/dev/null <<<"$OUT" || { WHY="a workspace cargo cannot read: exit 2 no_unit_graph, got $RC $(head -c 200 <<<"$OUT")"; return 1; }
  mkdir -p "$TMP/notcargo"; bk "$TMP/notcargo" --json
  [ "$RC" -eq 2 ] && jq -e '.ok==false and .code=="not_cargo_project"' >/dev/null <<<"$OUT" || { WHY="no Cargo.toml: exit 2 not_cargo_project, got $RC $(head -c 200 <<<"$OUT")"; return 1; }; }
c11() { mkdir -p "$TMP/profile/.hexa"; printf '{"build_keys":{"selections":["build -p native-sys","build -p app"]}}\n' > "$TMP/profile/.hexa/project.json"
  bk "$TMP/profile" --json; local rc=$RC; local sel; sel="$(jq -c .summary.selections <<<"$OUT")"
  rm -rf "$TMP/profile/.hexa"
  [ "$rc" -eq 1 ] && [ "$sel" = 2 ] || { WHY="project.json build_keys.selections replaces the defaults (2 selections, still unstable): rc $rc selections $sel"; return 1; }; }
c12() { local o; o="$(cd "$TMP/profile" && "$HEXA_BIN" dev build-keys --text 2>&1)"
  grep -q 'native-sys' <<<"$o" && grep -qi 'unstable' <<<"$o" && grep -q 'build-override' <<<"$o" \
    || { WHY="--text names native-sys, unstable and the hint: $(head -c 300 <<<"$o")"; return 1; }; }

# ---- phase 2: in hexa dev ci ---------------------------------------------------
c13() { local o
  mkdir -p "$TMP/profile/.hexa"; printf '{"build_keys":{"enabled":true}}\n' > "$TMP/profile/.hexa/project.json"
  o="$(cd "$TMP/profile" && "$HEXA_BIN" dev ci 2>&1)"
  rm -rf "$TMP/profile/.hexa"
  grep -i 'build keys' <<<"$o" | grep -qiE 'unstable|fail|✗' || { WHY="enabled: hexa dev ci must show a failing Build keys line: $(grep -i 'build keys' <<<"$o" | head -c 200)"; return 1; }
  o="$(cd "$TMP/profile" && "$HEXA_BIN" dev ci 2>&1)"
  grep -qi 'build keys' <<<"$o" && { WHY="not enabled: hexa dev ci must not mention build keys"; return 1; }
  return 0; }

check "1  an unstable native script: exit 1, summary"                       1 c1
check "2  default selections: workspace plus each member (9)"                1 c2
check "3  groups: host-only shared in one key, target-shared in the other"             1 c3
check "4  kind profile, hint names build-override, says prediction"          1 c4
check "5  kind features, hint names a workspace-hack build-dependency"       1 c5
check "6  the build-override fix makes it stable"                            1 c6
check "7  dev and test names are not a difference"                           1 c7
check "8  no native build script: exit 0 and says so"                        1 c8
check "9  one selection compares nothing: exit 2"                            1 c9
check "10 unreadable workspace and not-a-cargo-project: exit 2"              1 c10
check "11 .hexa/project.json build_keys.selections replaces the defaults"    1 c11
check "12 --text names the script, unstable and the hint"                    1 c12
check "13 hexa dev ci runs it only when build_keys.enabled"                  2 c13

echo "---"
[ "$RAN" -gt 0 ] || { echo "FAIL: no check ran"; exit 1; }
echo "$((RAN-FAILED)) of $RAN checks pass"
# The summary in the form `hexa adr gates` reads (TAP's `# tests`, `# pass`, `# fail`), so the reader
# counts the checks itself and fails a run that counted none, instead of this gate opting out with
# `## Gate (uncounted)`.
echo "# tests $RAN"; echo "# pass $((RAN-FAILED))"; echo "# fail $FAILED"
[ "$FAILED" -eq 0 ]
