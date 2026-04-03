#!/bin/bash
# Scan test files for FAIL(expected) — tests marked test_expect_failure that actually pass.
# These can be flipped to test_expect_success.
set -e
GRIT_BIN="${1:-$(pwd)/target/release/grit}"
TESTS_DIR="$(pwd)/tests"

total_flippable=0

for f in "$TESTS_DIR"/t*.sh; do
    # Skip files with no test_expect_failure
    grep -q "test_expect_failure" "$f" || continue
    
    base=$(basename "$f")
    # Run test and capture output
    output=$(cd "$TESTS_DIR" && GUST_BIN="$GRIT_BIN" timeout 60 sh "./$base" 2>&1) || true
    
    # Count FAIL(expected) entries
    count=$(echo "$output" | grep -c "FAIL(expected)" || true)
    
    if [ "$count" -gt 0 ]; then
        # Extract the test names
        names=$(echo "$output" | grep "FAIL(expected)" | sed 's/.*FAIL(expected) [0-9]*: /  /')
        echo "FLIP: $base — $count FAIL(expected)"
        echo "$names"
        total_flippable=$((total_flippable + count))
    fi
done

echo ""
echo "Total flippable: $total_flippable"
