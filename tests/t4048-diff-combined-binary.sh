#!/bin/sh

test_description='combined and merge diff handle binary files and textconv'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup merge with text conflict' '
	git init repo &&
	cd repo &&
	echo one >text &&
	git add text &&
	test_tick &&
	git commit -m one &&
	echo two >text &&
	git add text &&
	test_tick &&
	git commit -m two &&
	git checkout -b branch-text HEAD~1 &&
	echo three >text &&
	git add text &&
	test_tick &&
	git commit -m three &&
	test_must_fail git merge main
'

test_expect_success 'resolve conflict and commit' '
	cd repo &&
	echo resolved >text &&
	git add text &&
	git commit -m "resolved"
'

test_expect_success 'diff-tree shows changes in merge commit' '
	cd repo &&
	git diff-tree -r --name-only HEAD >actual &&
	grep "text" actual
'

test_expect_success 'diff-tree -p shows patch for merge' '
	cd repo &&
	git diff-tree -r -p HEAD >actual &&
	grep "text" actual
'

test_done
