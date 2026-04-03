#!/bin/sh
test_description='Tests for "git reset" with "--keep" and "--hard" options'
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success setup '
	printf "line %d\n" 1 2 3 >file1 &&
	cat file1 >file2 &&
	git add file1 file2 &&
	test_tick &&
	git commit -m "Initial commit" &&
	git tag initial &&
	echo line 4 >>file1 &&
	cat file1 >file2 &&
	git add file1 &&
	test_tick &&
	git commit -m "add line 4 to file1" &&
	git tag second
'

test_expect_success 'reset --keep fails with changes in index in files it touches' '
	git reset --hard second &&
	echo "line 5" >> file1 &&
	git add file1 &&
	test_must_fail git reset --keep HEAD^
'

test_expect_success 'reset --keep fails with changes in file it touches' '
	git reset --hard second &&
	echo "line 5" >> file1 &&
	git add file1 &&
	test_tick &&
	git commit -m "add line 5" &&
	sed -e "s/line 1/changed line 1/" <file1 >file3 &&
	mv file3 file1 &&
	test_must_fail git reset --keep HEAD^
'

test_expect_success 'reset --hard works' '
	git reset --hard second &&
	grep 4 file1 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	test -z "$(git diff --cached)" &&
	test -z "$(git diff)"
'

test_expect_success 'reset --hard to initial' '
	git reset --hard initial &&
	! grep 4 file1 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse initial)"
'

test_expect_success 'reset --soft preserves working tree and index' '
	git reset --hard second &&
	echo modified >file1 &&
	git add file1 &&
	git reset --soft initial &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse initial)" &&
	git diff --cached -- file1 >diff_out &&
	test -s diff_out
'

test_done
