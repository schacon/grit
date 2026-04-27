#!/usr/bin/env bash
# Run grit harness tests and update data/test-files.csv + dashboards.
#
# Usage:
#   ./scripts/run-tests.sh                     # all in-scope test files
#   ./scripts/run-tests.sh t1                  # all tests/t1*.sh (glob prefix; t1xxx family)
#   ./scripts/run-tests.sh t3200-branch.sh     # single file
#   ./scripts/run-tests.sh t0500 t4064 t4051   # multiple prefixes, .sh paths, or mixes (order preserved, deduped)
#   ./scripts/run-tests.sh --parallel          # all families t0–t9 in parallel (writes data/family/<d>.csv, then stitches)
#
# Options:
#   --timeout N    per-file timeout (default: 120)
#   --quiet        minimal output
#   --verbose, -v  print each test file as it starts ([i/N] name …) before the per-file result line
#   --from NAME    resume: skip tests before NAME (stem or .sh; first match in run order)
#   --parallel     run one process per Git test family (t0–t9); merge into data/test-files.csv at the end
#   --family N     only tests whose CSV group is tN (N is 0–9 or t0–t9)
#   --output-csv PATH   merge harness results into this CSV instead of data/test-files.csv (full catalog copy updated)
#   --no-catalog   skip generate-test-files-catalog.py (parent already refreshed the catalog)
#
# Skipped files (in_scope=skip in data/test-files.csv) are never run.
# After each test file finishes, its row in data/test-files.csv is updated;
# when the run completes, docs/progress/index.html + docs/testfiles.html are regenerated once.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
TESTS_DIR="$REPO/tests"
DATA_DIR="$REPO/data"
CSV="$DATA_DIR/test-files.csv"
CATALOG="$REPO/scripts/generate-test-files-catalog.py"
APPLY="$REPO/scripts/apply-test-run-results.py"
GEN_DASH="$REPO/scripts/generate-dashboard-from-test-files.py"
BIN="$REPO/target/release/grit"
TIMEOUT=120
QUIET=false
VERBOSE=false
TARGET=""
FROM=""
POS=()
PARALLEL=false
FAMILY=""
OUTPUT_CSV=""
NO_CATALOG=false

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
  --verbose|-v)
    VERBOSE=true
    shift
    ;;
  --parallel)
    PARALLEL=true
    shift
    ;;
  --family)
    if [[ $# -lt 2 ]]; then
      echo "ERROR: --family requires a digit 0-9 or t0-t9"
      exit 1
    fi
    FAMILY="$2"
    shift 2
    ;;
  --output-csv)
    if [[ $# -lt 2 ]]; then
      echo "ERROR: --output-csv requires a path"
      exit 1
    fi
    OUTPUT_CSV="$2"
    shift 2
    ;;
  --no-catalog)
    NO_CATALOG=true
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

if [[ "$VERBOSE" == true && "$QUIET" == true ]]; then
  echo "ERROR: --verbose and --quiet are mutually exclusive"
  exit 1
fi

if [[ "$PARALLEL" == true ]]; then
  PY_ARGS=(--timeout "$TIMEOUT")
  [[ "$QUIET" == true ]] && PY_ARGS+=(--quiet)
  [[ "$VERBOSE" == true ]] && PY_ARGS+=(--verbose)
  [[ -n "$FROM" ]] && PY_ARGS+=(--from "$FROM")
  exec python3 "$REPO/scripts/run-tests-parallel.py" "${PY_ARGS[@]}" -- "${POS[@]}"
fi

if [[ -n "$FAMILY" ]]; then
  case "$FAMILY" in
  t[0-9]|[0-9]) ;;
  *)
    echo "ERROR: --family must be a single digit 0-9 or t0-t9 (got: $FAMILY)"
    exit 1
    ;;
  esac
fi

if [[ -n "$OUTPUT_CSV" && "$OUTPUT_CSV" != /* ]]; then
  OUTPUT_CSV="$REPO/$OUTPUT_CSV"
fi
APPLY_CSV="${OUTPUT_CSV:-$CSV}"

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
if [[ "$NO_CATALOG" != true ]]; then
  python3 "$CATALOG"
fi

if [[ ! -f "$CSV" ]]; then
  echo "ERROR: $CSV was not created"
  exit 1
fi

if [[ -n "$OUTPUT_CSV" && "$OUTPUT_CSV" != "$CSV" ]]; then
  mkdir -p "$(dirname "$OUTPUT_CSV")"
  cp "$CSV" "$OUTPUT_CSV"
fi

if [[ -n "$FAMILY" ]]; then
  export GRIT_FAMILY_FILTER="$FAMILY"
else
  unset GRIT_FAMILY_FILTER 2>/dev/null || true
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

ff = os.environ.get("GRIT_FAMILY_FILTER", "").strip()
def canon_family(s):
    if not s:
        return ""
    if s.startswith("t") and len(s) >= 2 and s[1].isdigit():
        return "t" + s[1]
    if s.isdigit():
        return "t" + s[0]
    return ""

want = canon_family(ff)
if want:
    file_to_group = {}
    for row in rows:
        fn = row.get("file", "").strip()
        if fn:
            file_to_group[fn] = row.get("group", "").strip()
    candidates = [
        c
        for c in candidates
        if file_to_group.get(c[:-3] if c.endswith(".sh") else c, "") == want
    ]

for c in candidates:
    print(c)
PY
)

if [[ ${#FILES[@]} -eq 0 ]]; then
  echo "No test files to run (all skipped or no match)."
  if [[ -z "$OUTPUT_CSV" || "$OUTPUT_CSV" == "$CSV" ]]; then
    python3 "$GEN_DASH"
  fi
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
  # `command -v perl` can print a bare "perl"; shebangs require an absolute path (t5532-fetch-proxy).
  local perl_abs
  perl_abs="$(type -P perl 2>/dev/null || true)"
  [[ -z "$perl_abs" ]] && perl_abs=/usr/bin/perl
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
        -u GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME \
        -u TRASH_DIRECTORY -u BIN_DIRECTORY -u TEST_OUTPUT_DIRECTORY_OVERRIDE \
        EDITOR=: VISUAL=: LC_ALL=C LANG=C _prereq_DEFAULT_REPO_FORMAT=set \
        GRIT_TEST_LIB_SUMMARY=1 \
        ${utf8_nfd_to_nfc:+GIT_TEST_UTF8_NFD_TO_NFC=$utf8_nfd_to_nfc} \
        ${git_test_allow_sudo:+GIT_TEST_ALLOW_SUDO=$git_test_allow_sudo} \
        GUST_BIN="$BIN" \
        PERL_PATH="$perl_abs" \
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

for i in "${!FILES[@]}"; do
  f="${FILES[$i]}"
  if [[ "$VERBOSE" == true && "$QUIET" != true ]]; then
    printf '  [%d/%d] %s ...\n' "$((i + 1))" "${#FILES[@]}" "$f" >&2
  fi
  line=$(run_one "$f")
  printf '%s\n' "$line" >"$LINE_TMP"
  python3 "$APPLY" "$LINE_TMP" --skip-dashboard --csv "$APPLY_CSV"
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

if [[ -z "$OUTPUT_CSV" || "$OUTPUT_CSV" == "$CSV" ]]; then
  python3 "$GEN_DASH"
fi

if [[ "$QUIET" != true ]]; then
  if [[ -n "$OUTPUT_CSV" && "$OUTPUT_CSV" != "$CSV" ]]; then
    echo "Updated $APPLY_CSV (merge into main CSV with scripts/stitch-family-csvs.py if needed)."
  else
    echo "Updated $CSV and dashboards."
  fi
fi
