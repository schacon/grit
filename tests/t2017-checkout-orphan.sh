#!/bin/sh

test_description='git checkout --orphan (basic)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo "Initial" >foo &&
	git add foo &&
	git commit -m "First Commit" &&
	echo "State 1" >>foo &&
	git add foo &&
	git commit -m "Second Commit"
'

test_expect_success 'checkout -b creates a new branch from HEAD' '
	git checkout -b alpha &&
	test "refs/heads/alpha" = "$(git symbolic-ref HEAD)" &&
	git commit --allow-empty -m "Third Commit"
'

test_expect_success 'checkout back to main' '
	git checkout main &&
	test "refs/heads/main" = "$(git symbolic-ref HEAD)"
'

test_expect_success 'detached HEAD' '
	git checkout HEAD^0 &&
	test_must_fail git symbolic-ref HEAD
'

test_done
