#!/bin/sh

test_description='merging with symlink-like entries'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	>file &&
	git add file &&
	git commit -m initial &&
	git branch b-other &&
	echo main-content >file2 &&
	git add file2 &&
	git commit -m main &&
	git checkout b-other &&
	echo other-content >file3 &&
	git add file3 &&
	git commit -m other
'

test_expect_success 'merge non-overlapping changes' '
	cd repo &&
	git checkout main &&
	git merge b-other &&
	test_path_is_file file2 &&
	test_path_is_file file3
'

test_expect_success 'merge result has all files' '
	cd repo &&
	git ls-files -s >output &&
	test_line_count = 3 output
'

test_done
