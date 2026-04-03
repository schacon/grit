#!/bin/sh

test_description='Test detached HEAD operations'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	git commit --allow-empty -m initial &&
	git branch side &&
	git commit --allow-empty -m second
'

test_expect_success 'switch to branch' '
	git checkout side &&
	echo refs/heads/side >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'detach to self' '
	git checkout main &&
	git checkout main^0 &&
	test_must_fail git symbolic-ref HEAD
'

test_expect_success 'attach to branch from detached' '
	git checkout side &&
	echo refs/heads/side >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_done
