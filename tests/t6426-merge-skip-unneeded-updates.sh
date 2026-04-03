#!/bin/sh
# Ported from git/t/t6426-merge-skip-unneeded-updates.sh
# Tests that merge skips updating files when the result matches what's already there.

test_description='merge skips unneeded updates'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Testcase: Changes on one side, subset of changes on other
test_expect_success 'setup: simple merge with no conflicting changes' '
	git init merge-skip &&
	cd merge-skip &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo "base content" >file &&
	git add file &&
	git commit -m "base" &&
	git tag base &&

	git branch sideA &&
	git branch sideB &&

	git checkout sideA &&
	echo "new file on A" >fileA &&
	git add fileA &&
	git commit -m "add fileA" &&

	git checkout sideB &&
	echo "new file on B" >fileB &&
	git add fileB &&
	git commit -m "add fileB"
'

test_expect_success 'merge with no overlapping changes succeeds' '
	cd merge-skip &&
	git checkout sideA &&
	git merge sideB -m "merge B into A" &&
	test_path_is_file fileA &&
	test_path_is_file fileB &&
	test_path_is_file file
'

test_expect_success 'fast-forward merge does not create merge commit' '
	cd merge-skip &&
	git checkout base &&
	git merge sideA &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse sideA)"
'

test_done
