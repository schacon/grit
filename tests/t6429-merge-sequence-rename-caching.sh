#!/bin/sh
# Adapted from git/t/t6429-merge-sequence-rename-caching.sh
# Tests merge with sequential operations

test_description='merge with sequential operations'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: merge sequence with non-overlapping changes' '
	git init merge-seq &&
	cd merge-seq &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo "original content" >file1 &&
	echo "other content" >file2 &&
	git add file1 file2 &&
	git commit -m "initial" &&

	git branch modify-1 &&
	git branch modify-2 &&

	git checkout modify-1 &&
	echo "modified by 1" >>file1 &&
	git add file1 &&
	git commit -m "modify file1" &&

	git checkout modify-2 &&
	echo "modified by 2" >>file2 &&
	git add file2 &&
	git commit -m "modify file2"
'

test_expect_success 'merge non-overlapping changes succeeds' '
	cd merge-seq &&
	git checkout modify-1 &&
	git merge modify-2 -m "merge" &&
	test_grep "modified by 1" file1 &&
	test_grep "modified by 2" file2
'

test_expect_success 'sequential merges maintain history' '
	cd merge-seq &&
	git log --oneline >log &&
	test_line_count = 4 log
'

test_done
