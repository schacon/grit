#!/bin/sh

test_description='Merge-recursive rename/delete conflict message'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com"
'

test_expect_success 'rename/delete merge produces a merge commit' '
	echo foo >A &&
	git add A &&
	git commit -m "initial" &&

	git checkout -b rename &&
	git mv A B &&
	git commit -m "rename" &&

	git checkout main &&
	git rm A &&
	git commit -m "delete" &&

	git merge rename &&
	git log --oneline >log_output &&
	head -1 log_output | grep -i "merge"
'

test_done
