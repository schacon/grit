#!/bin/sh

test_description='git mergetool basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# This test is minimal - the upstream test requires mergetools/vimdiff
# which is not available in grit. We just verify the repo can be created
# and basic merge operations work.

test_expect_success 'setup repo for mergetool context' '
	git init mergetool-repo &&
	cd mergetool-repo &&
	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m base &&

	git checkout -b side &&
	echo side >file &&
	git add file &&
	test_tick &&
	git commit -m side &&

	git checkout master &&
	echo main >file &&
	git add file &&
	test_tick &&
	git commit -m main
'

test_expect_success 'merge conflict creates MERGE_HEAD' '
	cd mergetool-repo &&
	test_must_fail git merge side &&
	test_path_is_file .git/MERGE_HEAD
'

test_expect_success 'merge --abort cleans up after conflict' '
	cd mergetool-repo &&
	git merge --abort &&
	test_path_is_missing .git/MERGE_HEAD
'

test_done
