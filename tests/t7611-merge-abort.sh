#!/bin/sh

test_description='git merge --abort'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init merge-abort &&
	cd merge-abort &&
	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m base &&
	git tag base &&

	git checkout -b side &&
	echo side >file &&
	git add file &&
	test_tick &&
	git commit -m side &&

	git checkout master &&
	echo main-content >file &&
	git add file &&
	test_tick &&
	git commit -m main
'

test_expect_success 'merge --abort after conflict' '
	cd merge-abort &&
	git checkout master &&
	test_must_fail git merge side &&
	test_path_is_file .git/MERGE_HEAD &&
	git merge --abort &&
	test_path_is_missing .git/MERGE_HEAD &&
	echo main-content >expect &&
	test_cmp expect file
'

test_expect_success 'merge --abort restores working tree' '
	cd merge-abort &&
	git checkout master &&
	test_must_fail git merge side &&
	git merge --abort &&
	git diff --exit-code
'

test_done
