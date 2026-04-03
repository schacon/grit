#!/bin/sh

test_description='Test reflog basic operations'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo content >file &&
	git add file &&
	git commit -m one
'

test_expect_success 'log shows commits' '
	git log --oneline >actual &&
	grep "one" actual
'

test_expect_success 'log --format shows custom format' '
	git log --format="%H %s" >actual &&
	grep "one" actual
'

test_done
