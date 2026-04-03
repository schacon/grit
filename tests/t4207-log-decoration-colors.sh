#!/bin/sh

test_description='test "git log --decorate" output'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	test_commit A &&
	test_commit B &&
	git tag v1.0
'

test_expect_success 'log --decorate shows branch and tag' '
	git log --oneline --decorate >actual &&
	grep "main" actual &&
	grep "v1.0\|tag:" actual
'

test_expect_success 'log --decorate shows HEAD' '
	git log --oneline --decorate >actual &&
	grep "HEAD" actual
'

test_done
