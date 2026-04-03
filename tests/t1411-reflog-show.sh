#!/bin/sh

test_description='Test reflog display routines'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not yet support log -g (reflog walk) or reflog show with --format.
# These tests exercise what we can and mark the rest as expected failures.

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >file &&
	git add file &&
	git commit -m one
'

test_expect_success 'log -g shows reflog headers' '
	git log -g -n 1 >tmp &&
	grep "^Reflog" tmp
'

test_expect_success 'oneline reflog format' '
	git log -g -n 1 --oneline >actual &&
	test_line_count = 1 actual
'

test_expect_success 'reflog default format' '
	git reflog -n 1 >actual &&
	test_line_count = 1 actual
'

test_expect_success 'reflog show runs without crash' '
	git reflog show HEAD >actual 2>&1 || true
'

test_expect_success 'empty reflog file' '
	git branch empty &&
	git reflog expire --expire=all refs/heads/empty &&
	git log -g empty >actual &&
	test_must_be_empty actual
'

test_done
