#!/bin/sh

test_description='test combined/stat/moved interaction'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'set up history with a merge' '
	git init repo &&
	cd repo &&
	test_commit A &&
	test_commit B &&
	git checkout -b side HEAD^ &&
	test_commit C &&
	git merge -m M main &&
	test_commit D
'

test_expect_success 'log shows merge commit' '
	cd repo &&
	git log --oneline >actual &&
	grep "D" actual &&
	grep "M" actual &&
	grep "C" actual
'

test_expect_success 'diff-tree shows D commit changes' '
	cd repo &&
	git diff-tree --name-only HEAD >actual &&
	grep "D.t" actual
'

test_done
