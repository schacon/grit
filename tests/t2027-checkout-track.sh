#!/bin/sh

test_description='tests for git branch creation and checkout'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit one &&
	test_commit two
'

test_expect_success 'checkout -b creates a new branch' '
	git checkout -b branch1 main &&
	test "$(git symbolic-ref HEAD)" = refs/heads/branch1
'

test_expect_success 'checkout -b from a tag works' '
	git checkout -b branch2 one &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse one)"
'

test_expect_success 'checkout -b fails when branch already exists' '
	test_must_fail git checkout -b branch1
'

test_expect_success 'checkout specific commit' '
	git checkout main &&
	test "$(git symbolic-ref HEAD)" = refs/heads/main
'

test_done
