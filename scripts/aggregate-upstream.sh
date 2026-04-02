#!/bin/bash
# Aggregate results from /tmp/grit-upstream-results/*.out
RESULTS=${1:-/tmp/grit-upstream-results}

total_pass=0; total_fail=0; total_files=0; files_with_results=0; files_all_pass=0
for out in "$RESULTS"/*.out; do
  total_files=$((total_files + 1))
  p=$(grep -cE '^ok [0-9]' "$out" 2>/dev/null) || p=0
  f=$(grep -cE '^not ok [0-9]' "$out" 2>/dev/null) || f=0
  total_pass=$((total_pass + p))
  total_fail=$((total_fail + f))
  if [ $((p + f)) -gt 0 ]; then
    files_with_results=$((files_with_results + 1))
    if [ "$f" -eq 0 ]; then
      files_all_pass=$((files_all_pass + 1))
    fi
  fi
done
total=$((total_pass + total_fail))
echo "=== UPSTREAM TEST RESULTS ==="
echo "Files attempted: $total_files"
echo "Files producing TAP output: $files_with_results"
echo "Files where ALL tests pass: $files_all_pass"
echo "Tests: $total (pass: $total_pass, fail: $total_fail)"
if [ $total -gt 0 ]; then
  pct=$(python3 -c "print(f'{$total_pass * 100 / $total:.1f}%')")
  echo "Pass rate: $pct"
fi
