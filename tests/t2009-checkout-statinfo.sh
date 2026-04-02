#!/bin/sh
#
# Tests for 'grit checkout' preserving/updating stat info correctly.
# Verifies file size, content, and that the index stays consistent
# after various checkout operations.

test_description='grit checkout stat info and index consistency'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "line1" >file1 &&
	echo "line2" >file2 &&
	printf "exact-size-content\n" >sized &&
	mkdir -p sub &&
	echo "subfile" >sub/s.txt &&
	git add . &&
	git commit -m "initial" &&

	git checkout -b other &&
	echo "other-line1" >file1 &&
	echo "other-line2" >file2 &&
	printf "different-size\n" >sized &&
	echo "other-sub" >sub/s.txt &&
	echo "new-file" >new.txt &&
	git add . &&
	git commit -m "changes on other" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# checkout -- restores correct content and size
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- restores file content after modification' '
	cd repo &&
	echo "modified" >file1 &&
	git checkout -- file1 &&
	test "$(cat file1)" = "line1"
'

test_expect_success 'checkout -- restores correct file size' '
	cd repo &&
	echo "this is a much longer modification" >file1 &&
	git checkout -- file1 &&
	original_size=$(wc -c <file1) &&
	test "$original_size" = "6"
'

test_expect_success 'checkout -- file is identical to committed version' '
	cd repo &&
	echo "dirty" >sized &&
	git checkout -- sized &&
	test "$(cat sized)" = "exact-size-content"
'

# ---------------------------------------------------------------------------
# diff reports clean after checkout --
# ---------------------------------------------------------------------------
test_expect_success 'diff reports no changes after checkout --' '
	cd repo &&
	echo "dirty1" >file1 &&
	echo "dirty2" >file2 &&
	git checkout -- file1 file2 &&
	git diff --exit-code
'

test_expect_success 'update-index --refresh clean after checkout --' '
	cd repo &&
	echo "dirty" >file1 &&
	git checkout -- file1 &&
	git update-index --refresh &&
	git diff --exit-code
'

# ---------------------------------------------------------------------------
# checkout -- preserves other files untouched
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- one file does not touch others' '
	cd repo &&
	echo "dirty1" >file1 &&
	echo "dirty2" >file2 &&
	git checkout -- file1 &&
	test "$(cat file1)" = "line1" &&
	test "$(cat file2)" = "dirty2" &&
	git checkout -- file2
'

# ---------------------------------------------------------------------------
# Branch switch updates file content and size correctly
# ---------------------------------------------------------------------------
test_expect_success 'branch switch updates file content' '
	cd repo &&
	git checkout other &&
	test "$(cat file1)" = "other-line1" &&
	test "$(cat file2)" = "other-line2" &&
	git checkout master &&
	test "$(cat file1)" = "line1" &&
	test "$(cat file2)" = "line2"
'

test_expect_success 'branch switch updates file size' '
	cd repo &&
	master_size=$(wc -c <sized) &&
	git checkout other &&
	other_size=$(wc -c <sized) &&
	test "$master_size" != "$other_size" &&
	git checkout master &&
	back_size=$(wc -c <sized) &&
	test "$back_size" = "$master_size"
'

test_expect_success 'branch switch adds new files with correct content' '
	cd repo &&
	git checkout other &&
	test -f new.txt &&
	test "$(cat new.txt)" = "new-file" &&
	git checkout master &&
	test_path_is_missing new.txt
'

# ---------------------------------------------------------------------------
# diff after branch switch shows clean state
# ---------------------------------------------------------------------------
test_expect_success 'diff is clean after branch switch' '
	cd repo &&
	git checkout other &&
	git diff --exit-code &&
	git checkout master &&
	git diff --exit-code
'

test_expect_success 'diff is clean on other after branch switch' '
	cd repo &&
	git checkout other &&
	git diff --exit-code &&
	git diff --cached --exit-code &&
	git checkout master
'

# ---------------------------------------------------------------------------
# checkout <branch> -- <file> updates content correctly
# ---------------------------------------------------------------------------
test_expect_success 'checkout other -- file1 gets other branch content' '
	cd repo &&
	git checkout other -- file1 &&
	test "$(cat file1)" = "other-line1" &&
	git checkout master -- file1
'

test_expect_success 'checkout other -- file1 does not change file2 content' '
	cd repo &&
	git checkout other -- file1 &&
	test "$(cat file2)" = "line2" &&
	git checkout HEAD -- file1
'

# ---------------------------------------------------------------------------
# Repeated checkout -- is idempotent
# ---------------------------------------------------------------------------
test_expect_success 'repeated checkout -- is idempotent' '
	cd repo &&
	echo "dirty" >file1 &&
	git checkout -- file1 &&
	content1=$(cat file1) &&
	git checkout -- file1 &&
	content2=$(cat file1) &&
	test "$content1" = "$content2"
'

# ---------------------------------------------------------------------------
# checkout -- after partial staging
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- restores to staged version, not HEAD' '
	cd repo &&
	echo "staged" >file1 &&
	git add file1 &&
	echo "worktree" >file1 &&
	git checkout -- file1 &&
	test "$(cat file1)" = "staged" &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

test_expect_success 'diff shows no worktree changes after checkout -- with staged' '
	cd repo &&
	echo "staged" >file1 &&
	git add file1 &&
	echo "worktree" >file1 &&
	git checkout -- file1 &&
	git diff --exit-code &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

# ---------------------------------------------------------------------------
# checkout -f resets stat info
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f restores all files to clean state' '
	cd repo &&
	echo "d1" >file1 &&
	echo "d2" >file2 &&
	echo "d3" >sized &&
	git checkout -f master &&
	test "$(cat file1)" = "line1" &&
	test "$(cat file2)" = "line2" &&
	test "$(cat sized)" = "exact-size-content"
'

test_expect_success 'diff clean after checkout -f' '
	cd repo &&
	echo "dirty" >file1 &&
	git add file1 &&
	echo "more dirty" >file1 &&
	git checkout -f master &&
	git diff --exit-code &&
	git diff --cached --exit-code
'

# ---------------------------------------------------------------------------
# Subdirectory file stat consistency
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- sub/s.txt restores content' '
	cd repo &&
	echo "dirty-sub" >sub/s.txt &&
	git checkout -- sub/s.txt &&
	test "$(cat sub/s.txt)" = "subfile"
'

test_expect_success 'branch switch updates subdirectory files' '
	cd repo &&
	git checkout other &&
	test "$(cat sub/s.txt)" = "other-sub" &&
	git checkout master &&
	test "$(cat sub/s.txt)" = "subfile"
'

# ---------------------------------------------------------------------------
# Empty file handling
# ---------------------------------------------------------------------------
test_expect_success 'checkout handles empty file correctly' '
	cd repo &&
	>empty &&
	git add empty &&
	git commit -m "add empty file" &&
	echo "not empty" >empty &&
	git checkout -- empty &&
	test ! -s empty
'

# ---------------------------------------------------------------------------
# File with special content (binary-like)
# ---------------------------------------------------------------------------
test_expect_success 'checkout restores file with special characters' '
	cd repo &&
	printf "line1\tTabbed\nline2\n" >special &&
	git add special &&
	git commit -m "add special" &&
	echo "replaced" >special &&
	git checkout -- special &&
	printf "line1\tTabbed\nline2\n" >expected &&
	test_cmp expected special
'

# ---------------------------------------------------------------------------
# Multiple rapid checkout -- calls
# ---------------------------------------------------------------------------
test_expect_success 'rapid checkout -- calls all produce correct content' '
	cd repo &&
	echo d1 >file1 && git checkout -- file1 && test "$(cat file1)" = "line1" &&
	echo d2 >file1 && git checkout -- file1 && test "$(cat file1)" = "line1" &&
	echo d3 >file1 && git checkout -- file1 && test "$(cat file1)" = "line1" &&
	echo d4 >file1 && git checkout -- file1 && test "$(cat file1)" = "line1"
'

# ---------------------------------------------------------------------------
# checkout after reset --hard
# ---------------------------------------------------------------------------
test_expect_success 'status clean after reset --hard' '
	cd repo &&
	echo "dirty" >file1 &&
	git add file1 &&
	git reset --hard &&
	git diff --exit-code &&
	git diff --cached --exit-code &&
	test "$(cat file1)" = "line1"
'

test_done
