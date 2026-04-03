#!/bin/sh

test_description='update-index and ls-files with staged entries'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo initial >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'ls-files --stage shows staged entries' '
	git ls-files --stage >out &&
	test_line_count = 1 out &&
	grep "file$" out
'

test_expect_success 'update-index --refresh works' '
	git update-index --refresh
'

test_expect_success 'ls-files -u is empty with no unmerged entries' '
	git ls-files -u >out &&
	test_must_be_empty out
'

test_expect_success 'diff-files is quiet after refresh' '
	git diff-files --quiet
'

test_done
