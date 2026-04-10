#!/usr/bin/env bash
# Run grit harness tests and update data/test-files.csv + dashboards.
#
# Usage:
#   ./scripts/run-tests.sh                     # all in-scope test files
#   ./scripts/run-tests.sh t1                  # all tests/t1*.sh (glob prefix; t1xxx family)
#   ./scripts/run-tests.sh t3200-branch.sh     # single file
#   ./scripts/run-tests.sh t0500 t4064 t4051   # multiple prefixes, .sh paths, or mixes (order preserved, deduped)
#
# Options:
#   --timeout N    per-file timeout (default: 120)
#   --quiet        minimal output
#   --from NAME    resume: skip tests before NAME (stem or .sh; first match in run order)
#
# Skipped files (in_scope=skip in data/test-files.csv) are never run.
# After each test file finishes, its row in data/test-files.csv is updated;
# when the run completes, docs/index.html + docs/testfiles.html are regenerated once.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
TESTS_DIR="$REPO/tests"
DATA_DIR="$REPO/data"
CSV="$DATA_DIR/test-files.csv"
CATALOG="$REPO/scripts/generate-test-files-catalog.py"
APPLY="$REPO/scripts/apply-test-run-results.py"
GEN_DASH="$REPO/scripts/generate-dashboard-from-test-files.py"
BIN="$REPO/target/release/grit"
TIMEOUT=30
QUIET=false
TARGET=""
FROM=""
POS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
  --timeout)
    TIMEOUT="$2"
    shift 2
    ;;
  --quiet)
    QUIET=true
    shift
    ;;
  --from)
    if [[ $# -lt 2 ]]; then
      echo "ERROR: --from requires a test name (e.g. t1017-foo or t1017-foo.sh)"
      exit 1
    fi
    FROM="$2"
    shift 2
    ;;
  --)
    shift
    POS+=("$@")
    break
    ;;
  -*)
    echo "Unknown option: $1"
    exit 1
    ;;
  *)
    POS+=("$1")
    shift
    ;;
  esac
done

# GNU coreutils `timeout` is not installed by default on macOS; `gtimeout` may be.
# Built after parsing `--timeout` so the wrapper uses the final TIMEOUT value.
if command -v timeout >/dev/null 2>&1; then
  TIMEOUT_PREFIX=(timeout "$TIMEOUT")
elif command -v gtimeout >/dev/null 2>&1; then
  TIMEOUT_PREFIX=(gtimeout "$TIMEOUT")
else
  TIMEOUT_PREFIX=()
fi

if [[ ! -x "$BIN" ]]; then
  echo "ERROR: grit binary not found at $BIN"
  echo "Run: cargo build --release"
  exit 1
fi

rm -f "$TESTS_DIR/grit"
cp "$BIN" "$TESTS_DIR/grit"
chmod +x "$TESTS_DIR/grit"

mkdir -p "$DATA_DIR"
python3 "$CATALOG"

if [[ ! -f "$CSV" ]]; then
  echo "ERROR: $CSV was not created"
  exit 1
fi

# Build list of files to run: skip in_scope=skip
mapfile -t FILES < <(
  python3 - "$CSV" "$TESTS_DIR" "$FROM" "${POS[@]}" <<'PY'
import csv, os, sys, glob

csv_path, tests_dir, from_stem = sys.argv[1], sys.argv[2], sys.argv[3]
targets = sys.argv[4:]
if from_stem.endswith(".sh"):
    from_stem = from_stem[:-3]

rows = []
with open(csv_path, newline="") as f:
    r = csv.DictReader(f, delimiter="\t")
    for row in r:
        rows.append(row)

def want_file(base: str) -> bool:
    for row in rows:
        if row.get("file") == base:
            return row.get("in_scope", "yes").strip().lower() != "skip"
    return True


def normalize_target(raw: str) -> str:
    s = raw.strip()
    if not s:
        return s
    for prefix in ("tests/", "./tests/"):
        if s.startswith(prefix):
            s = s[len(prefix) :]
            break
    return os.path.basename(s)


def expand_one(target):
    out = []
    if target.endswith(".sh"):
        base = target[:-3]
        if want_file(base):
            p = os.path.join(tests_dir, target)
            if os.path.isfile(p):
                out.append(target)
        return out
    if target:
        for p in sorted(glob.glob(os.path.join(tests_dir, target + "*.sh"))):
            base = os.path.basename(p)[:-3]
            if want_file(base):
                out.append(os.path.basename(p))
        return out
    return out


candidates = []
if targets:
    seen = set()
    for raw in targets:
        t = normalize_target(raw)
        if not t:
            continue
        got = expand_one(t)
        if not got:
            print(
                "WARNING: no test files matched %r (skipped, typo, or missing under %s)"
                % (raw, tests_dir),
                file=sys.stderr,
            )
            continue
        for fn in got:
            if fn not in seen:
                seen.add(fn)
                candidates.append(fn)
else:
    for row in rows:
        if row.get("in_scope", "yes").strip().lower() == "skip":
            continue
        base = row.get("file", "")
        if not base:
            continue
        fn = base + ".sh"
        p = os.path.join(tests_dir, fn)
        if os.path.isfile(p):
            candidates.append(fn)

if from_stem:
    idx = None
    for i, c in enumerate(candidates):
        base = os.path.basename(c)
        stem = base[:-3] if base.endswith(".sh") else base
        if stem == from_stem:
            idx = i
            break
    if idx is None:
        print(
            "ERROR: --from %r: that test is not in this run list (wrong name, skipped, or no match)."
            % (from_stem,),
            file=sys.stderr,
        )
        sys.exit(1)
    candidates = candidates[idx:]

for c in candidates:
    print(c)
PY
)

if [[ ${#FILES[@]} -eq 0 ]]; then
  echo "No test files to run (all skipped or no match)."
  python3 "$GEN_DASH"
  exit 0
fi

RUN_NOTE=""
for _f in "${FILES[@]}"; do
  if [[ "$_f" == "t0410-partial-clone.sh" ]]; then
    RUN_NOTE=" (t0410-partial-clone.sh: no per-file timeout — long promisor/fetch suite)"
    break
  fi
done
[[ "$QUIET" != true ]] && echo "Running ${#FILES[@]} test file(s) (timeout: ${TIMEOUT}s)${RUN_NOTE}..."

LINE_TMP="$(mktemp)"
trap 'rm -f "$LINE_TMP"' EXIT

run_one() {
  local f="$1"
  local base="${f%.sh}"
  local output summary total pass fail status ef
  local git_test_allow_sudo=
  local utf8_nfd_to_nfc=
  local timeout_prefix=("${TIMEOUT_PREFIX[@]}")
  if [[ "$f" == "t0034-root-safe-directory.sh" ]]; then
    git_test_allow_sudo=YES
  fi
  # macOS NFC/NFD filesystem tests: force prereq on normal Linux CI (Git uses the same for portability).
  if [[ "$f" == "t3910-mac-os-precompose.sh" ]]; then
    utf8_nfd_to_nfc=1
  fi
  # t0410 can exceed any reasonable wall-clock cap on slow hosts; omit `timeout` so we still get # Tests: / TAP summary.
  if [[ "$f" == "t0410-partial-clone.sh" ]]; then
    timeout_prefix=()
  fi
  output=$(
    cd "$TESTS_DIR" &&
      # Cursor/agent shells often export `git () { ./grit "$@"; }`, which overrides the
      # harness `git` wrapper and breaks once a test `cd`s into trash (./grit missing).
      unset -f git grit 2>/dev/null || true &&
      env -u GIT_INDEX_FILE -u GIT_DIR -u GIT_WORK_TREE -u GIT_SEQUENCE_EDITOR \
        -u GIT_AUTHOR_DATE -u GIT_COMMITTER_DATE -u test_tick \
        EDITOR=: VISUAL=: LC_ALL=C LANG=C _prereq_DEFAULT_REPO_FORMAT=set \
        GRIT_TEST_LIB_SUMMARY=1 \
        ${utf8_nfd_to_nfc:+GIT_TEST_UTF8_NFD_TO_NFC=$utf8_nfd_to_nfc} \
        GUST_BIN="$BIN" \
        GIT_TEST_BUILTIN_HASH=sha1 \
        GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main \
        GIT_SOURCE_DIR="$REPO/git" \
        GIT_CONFIG_NOSYSTEM=1 \
        GIT_CONFIG_PARAMETERS= \
        "${timeout_prefix[@]}" bash "$f" 2>&1
  ) || true
  summary=$(echo "$output" | grep "^# Tests:" | tail -1) || true
  total=0 pass=0 fail=0 status="error"
  if [[ -n "$summary" ]]; then
    total=$(echo "$summary" | sed 's/.*Tests: \([0-9]*\).*/\1/')
    pass=$(echo "$summary" | sed 's/.*Pass: \([0-9]*\).*/\1/')
    fail=$(echo "$summary" | sed 's/.*Fail: \([0-9]*\).*/\1/')
    status="ok"
  else
    status="timeout"
  fi
  ef=$(grep -c 'test_expect_failure' "$TESTS_DIR/$f" 2>/dev/null || true)
  ef=${ef:-0}
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$base" "$total" "$pass" "$fail" "$status" "$ef"
}

for f in "${FILES[@]}"; do
  line=$(run_one "$f")
  printf '%s\n' "$line" >"$LINE_TMP"
  python3 "$APPLY" "$LINE_TMP" --skip-dashboard
  if [[ "$QUIET" != true ]]; then
    base="${f%.sh}"
    pass=$(echo "$line" | cut -f3)
    fail=$(echo "$line" | cut -f4)
    total=$(echo "$line" | cut -f2)
    if [[ "$fail" == "0" && "$total" != "0" ]]; then
      mark="✓"
    elif [[ "$total" == "0" ]]; then
      mark="⚠"
    else
      mark="✗"
    fi
    printf "  %s %s (%s/%s)\n" "$mark" "$base" "$pass" "$total"
  fi
done

python3 "$GEN_DASH"

if [[ "$QUIET" != true ]]; then
  echo "Updated $CSV and dashboards."
fi
