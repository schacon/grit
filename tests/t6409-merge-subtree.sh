#!/bin/sh

test_description='merge subtree basics'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	echo main >file &&
	git add file &&
	test_tick && git commit -m "main" &&
	git branch side &&
	echo main2 >file &&
	git add file &&
	test_tick && git commit -m "main2" &&
	git checkout side &&
	echo side >file2 &&
	git add file2 &&
	test_tick && git commit -m "side"
'

test_expect_success 'simple merge between branches' '
	cd repo &&
	git checkout main &&
	git merge side &&
	test_path_is_file file &&
	test_path_is_file file2
'

test_done
