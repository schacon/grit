#!/bin/sh

test_description='test cherry-picking an empty commit'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo first >file1 &&
	git add file1 &&
	test_tick &&
	git commit -m "first" &&

	git checkout -b empty-message-branch &&
	echo third >>file1 &&
	git add file1 &&
	test_tick &&
	git commit --allow-empty-message -m "" &&

	git checkout main
'

test_expect_success 'cherry-pick a commit with an empty message' '
	git reset --hard main &&
	git cherry-pick empty-message-branch
'

test_expect_success 'index lockfile was removed after cherry-pick' '
	test ! -f .git/index.lock
'

test_expect_success 'cherry-pick applies changes from empty-message commit' '
	echo first >expect &&
	echo third >>expect &&
	test_cmp expect file1
'

test_done
