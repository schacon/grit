#!/bin/sh

test_description='git add respects ignore patterns and explicit pathspec'

. ./test-lib.sh

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo content >tracked.txt &&
	git add tracked.txt &&
	git commit -m "initial"
'

test_expect_success 'add ignores files matching .gitignore' '
	echo "*.log" >.gitignore &&
	echo log_content >test.log &&
	test_must_fail git add test.log &&
	git ls-files >out &&
	! grep "test.log" out
'

test_expect_success 'add --force adds ignored files' '
	git add --force test.log &&
	git ls-files >out &&
	grep "test.log" out
'

test_expect_success 'status shows tracked ignored file changes' '
	echo changed >test.log &&
	git status --porcelain >out &&
	grep "test.log" out
'

test_done
