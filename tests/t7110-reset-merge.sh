#!/bin/sh
#
# Copyright (c) 2009 Christian Couder
#

test_description='Tests for "git reset" with "--merge" and "--keep" options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not yet implement reset --merge or --keep,
# so all tests are marked as expected failures.

test_expect_success setup '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	printf "line %d\n" 1 2 3 >file1 &&
	cat file1 >file2 &&
	git add file1 file2 &&
	test_tick &&
	git commit -m "Initial commit" &&
	git tag initial &&
	echo line 4 >>file1 &&
	cat file1 >file2 &&
	test_tick &&
	git commit -a -m "add line 4 to file1" &&
	git tag second
'

test_expect_failure 'reset --merge is ok with changes in file it does not touch' '
	cd repo &&
	git reset --merge HEAD^ &&
	! grep 4 file1 &&
	grep 4 file2 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse initial)" &&
	test -z "$(git diff --cached)"
'

test_expect_failure 'reset --merge is ok when switching back' '
	cd repo &&
	git reset --hard second &&
	git reset --merge second &&
	grep 4 file1 &&
	grep 4 file2 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	test -z "$(git diff --cached)"
'

test_expect_failure 'reset --keep is ok with changes in file it does not touch' '
	cd repo &&
	git reset --hard second &&
	cat file1 >file2 &&
	git reset --keep HEAD^ &&
	! grep 4 file1 &&
	grep 4 file2 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse initial)" &&
	test -z "$(git diff --cached)"
'

test_expect_failure 'reset --keep is ok when switching back' '
	cd repo &&
	git reset --keep second &&
	grep 4 file1 &&
	grep 4 file2 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	test -z "$(git diff --cached)"
'

test_expect_failure 'reset --merge discards changes added to index (1)' '
	cd repo &&
	git reset --hard second &&
	cat file1 >file2 &&
	echo "line 5" >> file1 &&
	git add file1 &&
	git reset --merge HEAD^ &&
	! grep 4 file1 &&
	! grep 5 file1 &&
	grep 4 file2 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse initial)" &&
	test -z "$(git diff --cached)"
'

test_expect_failure 'reset --merge is ok again when switching back (1)' '
	cd repo &&
	git reset --hard initial &&
	echo "line 5" >> file2 &&
	git add file2 &&
	git reset --merge second &&
	! grep 4 file2 &&
	! grep 5 file1 &&
	grep 4 file1 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	test -z "$(git diff --cached)"
'

test_expect_failure 'reset --keep fails with changes in index in files it touches' '
	cd repo &&
	git reset --hard second &&
	echo "line 5" >> file1 &&
	git add file1 &&
	test_must_fail git reset --keep HEAD^
'

test_expect_failure 'reset --merge discards changes added to index (2)' '
	cd repo &&
	git reset --hard second &&
	echo "line 4" >> file2 &&
	git add file2 &&
	git reset --merge HEAD^ &&
	! grep 4 file2 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse initial)" &&
	test -z "$(git diff)" &&
	test -z "$(git diff --cached)"
'

test_expect_failure 'reset --merge is ok again when switching back (2)' '
	cd repo &&
	git reset --hard initial &&
	git reset --merge second &&
	! grep 4 file2 &&
	grep 4 file1 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	test -z "$(git diff --cached)"
'

test_expect_failure 'reset --keep keeps changes it does not touch' '
	cd repo &&
	git reset --hard second &&
	echo "line 4" >> file2 &&
	git add file2 &&
	git reset --keep HEAD^ &&
	grep 4 file2 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse initial)" &&
	test -z "$(git diff --cached)"
'

test_expect_failure 'reset --keep keeps changes when switching back' '
	cd repo &&
	git reset --keep second &&
	grep 4 file2 &&
	grep 4 file1 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	test -z "$(git diff --cached)"
'

test_expect_failure 'reset --merge fails with changes in file it touches' '
	cd repo &&
	git reset --hard second &&
	echo "line 5" >> file1 &&
	test_tick &&
	git commit -a -m "add line 5" &&
	sed -e "s/line 1/changed line 1/" <file1 >file3 &&
	mv file3 file1 &&
	test_must_fail git reset --merge HEAD^
'

test_expect_failure 'reset --keep fails with changes in file it touches' '
	cd repo &&
	git reset --hard second &&
	echo "line 5" >> file1 &&
	test_tick &&
	git commit -a -m "add line 5" &&
	sed -e "s/line 1/changed line 1/" <file1 >file3 &&
	mv file3 file1 &&
	test_must_fail git reset --keep HEAD^
'

test_done
