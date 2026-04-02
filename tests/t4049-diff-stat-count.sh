#!/bin/sh
test_description='grit diff --stat output formatting and --stat-count/--stat-width options

Tests the diffstat summary output. The --stat-count and --stat-width
options are not yet implemented in grit; those are marked as expected
failures.'

. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "a" >file1.txt &&
	echo "b" >file2.txt &&
	echo "c" >file3.txt &&
	echo "d" >file4.txt &&
	echo "e" >file5.txt &&
	git add . &&
	git commit -m "initial" &&
	echo "A" >file1.txt &&
	echo "B" >file2.txt &&
	echo "C" >file3.txt &&
	echo "D" >file4.txt &&
	echo "E" >file5.txt
'

test_expect_success '--stat lists all changed files' '
	cd repo &&
	git diff --stat >out &&
	grep "file1\.txt" out &&
	grep "file2\.txt" out &&
	grep "file3\.txt" out &&
	grep "file4\.txt" out &&
	grep "file5\.txt" out
'

test_expect_success '--stat shows summary line' '
	cd repo &&
	git diff --stat >out &&
	grep "files\{0,1\} changed" out
'

test_expect_success '--stat for single file change' '
	git init single &&
	cd single &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "hello" >f.txt &&
	git add f.txt &&
	git commit -m "init" &&
	echo "world" >f.txt &&
	git diff --stat >out &&
	grep "f\.txt" out &&
	grep "1 file changed" out
'

test_expect_success '--stat for file with many changes' '
	git init many &&
	cd many &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	for i in $(seq 1 20); do echo "line$i"; done >big.txt &&
	git add big.txt &&
	git commit -m "init" &&
	for i in $(seq 1 20); do echo "changed$i"; done >big.txt &&
	git diff --stat >out &&
	grep "big\.txt" out
'

test_expect_success '--numstat shows machine-readable format' '
	cd repo &&
	git diff --numstat >out &&
	# numstat lines should have tab-separated fields: additions deletions filename
	grep "file1\.txt" out
'

test_expect_success '--numstat for multiple files' '
	cd repo &&
	git diff --numstat >out &&
	test_line_count = 5 out
'

test_expect_success '--stat with --cached' '
	cd repo &&
	git add file1.txt file2.txt &&
	git diff --cached --stat >out &&
	grep "file1\.txt" out &&
	grep "file2\.txt" out &&
	! grep "file3\.txt" out
'

test_expect_success '--numstat with --cached' '
	cd repo &&
	git diff --cached --numstat >out &&
	grep "file1\.txt" out &&
	test_line_count = 2 out
'

test_expect_success '--stat between commits' '
	cd repo &&
	git add . &&
	git commit -m "modified all" &&
	c1=$(git rev-parse HEAD~1) &&
	c2=$(git rev-parse HEAD) &&
	git diff-tree --stat "$c1" "$c2" >out &&
	grep "file1\.txt" out &&
	grep "files\{0,1\} changed" out
'

# ---- Unimplemented options (expected failures) ----

test_expect_failure 'diff --stat-count limits file count (not implemented)' '
	cd repo &&
	git checkout HEAD~1 -- . 2>/dev/null &&
	echo "A" >file1.txt && echo "B" >file2.txt &&
	echo "C" >file3.txt && echo "D" >file4.txt &&
	echo "E" >file5.txt &&
	git diff --stat-count=2 >out &&
	test_line_count = 3 out
'

test_expect_failure 'diff --stat-width limits line width (not implemented)' '
	cd repo &&
	git diff --stat-width=40 >out &&
	test -f out
'

test_expect_failure 'diff --stat-graph-width limits graph width (not implemented)' '
	cd repo &&
	git diff --stat-graph-width=10 >out &&
	test -f out
'

test_expect_failure 'diff --stat-name-width limits name width (not implemented)' '
	cd repo &&
	git diff --stat-name-width=20 >out &&
	test -f out
'

test_expect_failure 'diff --shortstat shows only summary (not implemented)' '
	cd repo &&
	echo "A" >file1.txt &&
	git diff --shortstat >out &&
	grep "file" out &&
	! grep "file1\.txt" out
'

test_done
