#!/bin/sh

test_description='git log --graph with merge history'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&

	test_tick &&
	echo A >file &&
	git add file &&
	git commit -q -m "A" &&

	git checkout -b side &&
	test_tick &&
	echo B >file &&
	git add file &&
	git commit -q -m "B" &&

	git checkout main &&
	test_tick &&
	echo C >file2 &&
	git add file2 &&
	git commit -q -m "C"
'

test_expect_success 'log --graph shows linear history' '
	git log --graph --oneline main >actual &&
	test_line_count -gt 0 actual
'

test_expect_success 'log --graph --format=%s' '
	git log --graph --format="%s" main >actual &&
	grep "C" actual &&
	grep "A" actual
'

test_expect_success 'log with merge' '
	git merge --no-ff side -m "Merge side" &&
	git log --graph --oneline >actual &&
	test_line_count -gt 3 actual
'

test_expect_success 'log --graph with --reverse' '
	git log --reverse --format="%s" >actual &&
	head -1 actual | grep "A"
'

test_expect_success 'log --graph with -n limit' '
	git log --graph -n 2 --oneline >actual &&
	test_line_count -le 4 actual
'

test_done
