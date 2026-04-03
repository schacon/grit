#!/bin/sh

test_description='git log --graph with skewed merges'

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

	test_tick &&
	echo B >file &&
	git add file &&
	git commit -q -m "B" &&

	git checkout -b feature HEAD^ &&
	test_tick &&
	echo C >file2 &&
	git add file2 &&
	git commit -q -m "C" &&

	git checkout main &&
	git merge --no-ff feature -m "merge feature"
'

test_expect_success 'log --graph shows merge structure' '
	git log --graph --format="%s" >actual &&
	grep "merge feature" actual &&
	grep "B" actual &&
	grep "C" actual &&
	grep "A" actual
'

test_expect_success 'log --graph --oneline' '
	git log --graph --oneline >actual &&
	test_line_count -gt 3 actual
'

test_expect_success 'log --first-parent shows only main line' '
	git log --first-parent --format="%s" >actual &&
	grep "merge feature" actual &&
	grep "B" actual &&
	grep "A" actual &&
	! grep "C" actual
'

test_expect_success 'log --no-merges skips merge commits' '
	git log --no-merges --format="%s" >actual &&
	! grep "merge feature" actual &&
	grep "B" actual &&
	grep "C" actual
'

test_expect_success 'log --merges shows only merge commits' '
	git log --merges --format="%s" >actual &&
	grep "merge feature" actual &&
	! grep "^B$" actual
'

test_done
