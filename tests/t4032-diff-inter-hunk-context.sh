#!/bin/sh
#
# Tests for diff inter-hunk context merging.
# When two changed regions are close enough that their context lines
# overlap, they should be merged into a single hunk. Varying -U values
# control whether hunks stay separate or merge.

test_description='grit diff — inter-hunk context merging'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Helper: generate a file with N lines ("line1" … "lineN")
# ---------------------------------------------------------------------------
gen_lines () {
	local n=$1
	local i=1
	while test $i -le $n; do
		echo "line$i"
		i=$(($i + 1))
	done
}

# ---------------------------------------------------------------------------
# Setup: each scenario creates a tag pair (before/after) on a branch
# ---------------------------------------------------------------------------
test_expect_success 'setup repo' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	gen_lines 40 >file.txt &&
	git add file.txt &&
	git commit -m "initial"
'

# --- Scenario 1: two changes far apart (lines 5 and 35) ---
test_expect_success 'scenario 1: commit with changes at lines 5 and 35' '
	cd repo &&
	git tag sc1-before &&
	gen_lines 40 | sed "s/^line5$/MOD5/" | sed "s/^line35$/MOD35/" >file.txt &&
	git add file.txt &&
	git commit -m "sc1" &&
	git tag sc1-after
'

test_expect_success 'sc1: diff -U3 shows two separate hunks' '
	cd repo &&
	git diff -U3 sc1-before sc1-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 2
'

test_expect_success 'sc1: diff -U0 shows two separate hunks' '
	cd repo &&
	git diff -U0 sc1-before sc1-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 2
'

test_expect_success 'sc1: diff -U0 hunks are minimal' '
	cd repo &&
	git diff -U0 sc1-before sc1-after >out &&
	grep "^-line5$" out &&
	grep "^+MOD5$" out &&
	grep "^-line35$" out &&
	grep "^+MOD35$" out
'

test_expect_success 'sc1: diff -U20 merges into one hunk' '
	cd repo &&
	git diff -U20 sc1-before sc1-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 1
'

test_expect_success 'sc1: merged hunk contains both changes' '
	cd repo &&
	git diff -U20 sc1-before sc1-after >out &&
	grep "MOD5" out &&
	grep "MOD35" out
'

# --- Scenario 2: changes 4 lines apart (10 and 14) — U3 merges ---
test_expect_success 'scenario 2: reset and commit with lines 10 and 14 changed' '
	cd repo &&
	git reset --hard sc1-before &&
	git tag sc2-before &&
	gen_lines 40 | sed "s/^line10$/NEAR10/" | sed "s/^line14$/NEAR14/" >file.txt &&
	git add file.txt &&
	git commit -m "sc2" &&
	git tag sc2-after
'

test_expect_success 'sc2: diff -U3 merges nearby hunks into one' '
	cd repo &&
	git diff -U3 sc2-before sc2-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 1
'

test_expect_success 'sc2: merged hunk contains both changes' '
	cd repo &&
	git diff -U3 sc2-before sc2-after >out &&
	grep "NEAR10" out &&
	grep "NEAR14" out
'

test_expect_success 'sc2: diff -U0 keeps them as two hunks' '
	cd repo &&
	git diff -U0 sc2-before sc2-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 2
'

test_expect_success 'sc2: diff -U1 keeps two hunks (gap=3 > 2*1)' '
	cd repo &&
	git diff -U1 sc2-before sc2-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 2
'

# --- Scenario 3: consecutive lines (15 and 16) ---
test_expect_success 'scenario 3: change consecutive lines 15 and 16' '
	cd repo &&
	git reset --hard sc1-before &&
	git tag sc3-before &&
	gen_lines 40 | sed "s/^line15$/ADJ15/" | sed "s/^line16$/ADJ16/" >file.txt &&
	git add file.txt &&
	git commit -m "sc3" &&
	git tag sc3-after
'

test_expect_success 'sc3: diff -U0 shows one hunk for consecutive changes' '
	cd repo &&
	git diff -U0 sc3-before sc3-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 1
'

test_expect_success 'sc3: diff -U3 shows one hunk' '
	cd repo &&
	git diff -U3 sc3-before sc3-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 1
'

test_expect_success 'sc3: both adjacent changes appear in diff' '
	cd repo &&
	git diff sc3-before sc3-after >out &&
	grep "ADJ15" out &&
	grep "ADJ16" out
'

# --- Scenario 4: three changes far apart (5, 20, 35) ---
test_expect_success 'scenario 4: three changes at lines 5, 20, 35' '
	cd repo &&
	git reset --hard sc1-before &&
	git tag sc4-before &&
	gen_lines 40 | sed "s/^line5$/TRI5/" | sed "s/^line20$/TRI20/" | sed "s/^line35$/TRI35/" >file.txt &&
	git add file.txt &&
	git commit -m "sc4" &&
	git tag sc4-after
'

test_expect_success 'sc4: diff -U3 shows three separate hunks' '
	cd repo &&
	git diff -U3 sc4-before sc4-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 3
'

test_expect_success 'sc4: all three changes present' '
	cd repo &&
	git diff -U3 sc4-before sc4-after >out &&
	grep "TRI5" out &&
	grep "TRI20" out &&
	grep "TRI35" out
'

test_expect_success 'sc4: diff -U10 merges into fewer hunks' '
	cd repo &&
	git diff -U10 sc4-before sc4-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -le 2
'

test_expect_success 'sc4: diff -U20 merges all into one' '
	cd repo &&
	git diff -U20 sc4-before sc4-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 1
'

# --- Scenario 5: boundary changes (first and last lines) ---
test_expect_success 'scenario 5: modify first and last lines' '
	cd repo &&
	git reset --hard sc1-before &&
	git tag sc5-before &&
	gen_lines 40 | sed "s/^line1$/FIRST/" | sed "s/^line40$/LAST/" >file.txt &&
	git add file.txt &&
	git commit -m "sc5" &&
	git tag sc5-after
'

test_expect_success 'sc5: diff -U3 at boundaries shows two hunks' '
	cd repo &&
	git diff -U3 sc5-before sc5-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 2
'

test_expect_success 'sc5: diff -U100 merges boundary hunks' '
	cd repo &&
	git diff -U100 sc5-before sc5-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 1
'

# --- Scenario 6: --stat and --numstat unaffected by context ---
test_expect_success 'sc1: diff --stat shows correct summary' '
	cd repo &&
	git diff --stat sc1-before sc1-after >out &&
	grep "file.txt" out
'

test_expect_success 'sc1: diff --numstat shows correct counts' '
	cd repo &&
	git diff --numstat sc1-before sc1-after >out &&
	grep "file.txt" out
'

# --- Scenario 7: cached diff with context ---
test_expect_success 'scenario 7: staged changes at 8 and 32' '
	cd repo &&
	git reset --hard sc1-before &&
	gen_lines 40 | sed "s/^line8$/STAGED8/" | sed "s/^line32$/STAGED32/" >file.txt &&
	git add file.txt
'

test_expect_success 'sc7: diff --cached -U3 shows two hunks' '
	cd repo &&
	git diff --cached -U3 >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 2
'

test_expect_success 'sc7: diff --cached -U15 merges hunks' '
	cd repo &&
	git diff --cached -U15 >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 1
'

# --- Scenario 8: two files, independent hunks ---
test_expect_success 'scenario 8: two separate files' '
	cd repo &&
	git reset --hard sc1-before &&
	gen_lines 20 >a.txt &&
	gen_lines 20 >b.txt &&
	git add a.txt b.txt &&
	git commit -m "two files" &&
	git tag sc8-before &&

	sed "s/^line3$/AMOD3/" a.txt >a.tmp && mv a.tmp a.txt &&
	sed "s/^line7$/BMOD7/" b.txt >b.tmp && mv b.tmp b.txt &&
	git add a.txt b.txt &&
	git commit -m "modify each" &&
	git tag sc8-after
'

test_expect_success 'sc8: diff -U3 shows one hunk per file' '
	cd repo &&
	git diff -U3 sc8-before sc8-after >out &&
	count=$(grep -c "^@@" out) &&
	test "$count" -eq 2
'

test_expect_success 'sc8: both file headers present' '
	cd repo &&
	git diff sc8-before sc8-after >out &&
	grep "^--- a/a.txt" out &&
	grep "^--- a/b.txt" out
'

test_done
