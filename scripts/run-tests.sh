#!/bin/bash
# Run grit test files and cache results in data/file-results.tsv
#
# Usage:
#   ./scripts/run-tests.sh                     # full suite (all categories in order)
#   ./scripts/run-tests.sh t1                   # category t1xxx
#   ./scripts/run-tests.sh t3070-wildmatch.sh   # single file
#   ./scripts/run-tests.sh --stale              # re-run files with no cached results
#   ./scripts/run-tests.sh --failing            # re-run files that aren't fully passing
#
# Options:
#   --timeout N    per-file timeout in seconds (default: 120)
#   --force        re-run even if cached results exist
#   --quiet        minimal output
#
# Results are written incrementally to data/file-results.tsv.
# Each run updates only the files that were tested.
# Format matches what generate-progress-html.py and generate-testfiles-html.py expect.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
TESTS_DIR="$REPO/tests"
DATA_DIR="$REPO/data"
RESULTS_FILE="$DATA_DIR/file-results.tsv"
BIN="$REPO/target/release/grit"
TIMEOUT=120
FORCE=false
QUIET=false
MODE=""
TARGET=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --timeout) TIMEOUT="$2"; shift 2 ;;
        --force) FORCE=true; shift ;;
        --quiet) QUIET=true; shift ;;
        --stale) MODE="stale"; shift ;;
        --failing) MODE="failing"; shift ;;
        t[0-9]*.sh) TARGET="$1"; shift ;;
        t[0-9]) TARGET="$1"; shift ;;
        t[0-9][0-9]) TARGET="$1"; shift ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

# Ensure binary exists
if [[ ! -x "$BIN" ]]; then
    echo "ERROR: grit binary not found at $BIN"
    echo "Run: cargo build --release"
    exit 1
fi

# Copy binary to tests dir
rm -f "$TESTS_DIR/grit"
cp "$BIN" "$TESTS_DIR/grit"
chmod +x "$TESTS_DIR/grit"

# Ensure data dir and results file exist with correct header
mkdir -p "$DATA_DIR"
HEADER="file	ported	total_tests	passing	failing	status	real_pass	real_total	expect_failure"
if [[ ! -f "$RESULTS_FILE" ]]; then
    echo "$HEADER" > "$RESULTS_FILE"
fi

# Load existing results into associative array (key=file_base, value=rest of line)
declare -A CACHED
while IFS= read -r line; do
    [[ "$line" == file* ]] && continue
    key="${line%%	*}"
    val="${line#*	}"
    CACHED["$key"]="$val"
done < "$RESULTS_FILE"

# Determine which files to run
get_test_files() {
    if [[ -n "$TARGET" && "$TARGET" == *.sh ]]; then
        echo "$TARGET"
    elif [[ -n "$TARGET" ]]; then
        ls "$TESTS_DIR"/${TARGET}*.sh 2>/dev/null | xargs -n1 basename | sort
    else
        ls "$TESTS_DIR"/t[0-9]*.sh 2>/dev/null | xargs -n1 basename | sort
    fi
}

FILES=()
while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    base="${f%.sh}"

    if [[ "$MODE" == "stale" ]]; then
        [[ -z "${CACHED[$base]:-}" ]] && FILES+=("$f")
    elif [[ "$MODE" == "failing" ]]; then
        cached="${CACHED[$base]:-}"
        if [[ -z "$cached" ]]; then
            FILES+=("$f")
        else
            fail=$(echo "$cached" | cut -f4)
            [[ "$fail" != "0" ]] && FILES+=("$f")
        fi
    elif [[ "$FORCE" == true ]]; then
        FILES+=("$f")
    else
        FILES+=("$f")
    fi
done < <(get_test_files)

if [[ ${#FILES[@]} -eq 0 ]]; then
    echo "No test files to run."
    exit 0
fi

[[ "$QUIET" != true ]] && echo "Running ${#FILES[@]} test files (timeout: ${TIMEOUT}s)..."

# Run a single test file
run_one() {
    local f="$1"
    local base="${f%.sh}"

    local output
    output=$(
        cd "$TESTS_DIR" &&
            env -u GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME \
                EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="$(pwd)/grit" \
                timeout "$TIMEOUT" bash "$f" 2>&1
    ) || true
    local summary
    summary=$(echo "$output" | grep "^# Tests:" | tail -1)

    local total=0 pass=0 fail=0 status="error"
    if [[ -n "$summary" ]]; then
        total=$(echo "$summary" | sed 's/.*Tests: \([0-9]*\).*/\1/')
        pass=$(echo "$summary" | sed 's/.*Pass: \([0-9]*\).*/\1/')
        fail=$(echo "$summary" | sed 's/.*Fail: \([0-9]*\).*/\1/')
        status="ok"
    else
        status="timeout"
    fi

    # Count test_expect_failure stubs
    local ef
    ef=$(grep -c 'test_expect_failure' "$TESTS_DIR/$f" 2>/dev/null | tail -1)
    ef=${ef:-0}
    local real_pass=$((pass > ef ? pass - ef : 0))
    local real_total=$((total > ef ? total - ef : total))

    printf '%s\tyes\t%d\t%d\t%d\t%s\t%d\t%d\t%d\n' \
        "$base" "$total" "$pass" "$fail" "$status" "$real_pass" "$real_total" "$ef"
}

# Run all files sequentially
RESULTS=()
COMPLETED=0
TOTAL=${#FILES[@]}

for f in "${FILES[@]}"; do
    line=$(run_one "$f")
    RESULTS+=("$line")
    COMPLETED=$((COMPLETED + 1))
    if [[ "$QUIET" != true ]]; then
        base="${f%.sh}"
        pass=$(echo "$line" | cut -f4)
        fail=$(echo "$line" | cut -f5)
        total=$(echo "$line" | cut -f3)
        if [[ "$fail" == "0" && "$total" != "0" ]]; then
            mark="✓"
        elif [[ "$total" == "0" ]]; then
            mark="⚠"
        else
            mark="✗"
        fi
        printf "\r  %s %d/%d  %s (%s/%s)          \n" "$mark" "$COMPLETED" "$TOTAL" "$base" "$pass" "$total"
    fi
done

# Update cache with new results
for line in "${RESULTS[@]}"; do
    key="${line%%	*}"
    val="${line#*	}"
    CACHED["$key"]="$val"
done

# Write results file atomically
TMP="$RESULTS_FILE.tmp.$$"
echo "$HEADER" > "$TMP"
for key in $(printf '%s\n' "${!CACHED[@]}" | sort); do
    printf '%s\t%s\n' "$key" "${CACHED[$key]}" >> "$TMP"
done
mv "$TMP" "$RESULTS_FILE"

# Summary
total_pass=0 total_fail=0 total_tests=0 pass_files=0 fail_files=0
for line in "${RESULTS[@]}"; do
    t=$(echo "$line" | cut -f3)
    p=$(echo "$line" | cut -f4)
    fl=$(echo "$line" | cut -f5)
    total_tests=$((total_tests + t))
    total_pass=$((total_pass + p))
    total_fail=$((total_fail + fl))
    if [[ "$fl" -eq 0 && "$t" -gt 0 ]]; then
        pass_files=$((pass_files + 1))
    elif [[ "$t" -gt 0 ]]; then
        fail_files=$((fail_files + 1))
    fi
done

if [[ "$total_tests" -gt 0 ]]; then
    pct=$((total_pass * 100 / total_tests))
    [[ "$QUIET" != true ]] && echo ""
    [[ "$QUIET" != true ]] && echo "Results: $total_pass/$total_tests tests ($pct%) — $pass_files fully passing, $fail_files partial"
fi

[[ "$QUIET" != true ]] && echo "Updated $RESULTS_FILE"
