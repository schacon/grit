#!/bin/sh

test_description='various tests of reflog walk (log -g) behavior'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "t@t" &&
	test_commit one &&
	test_commit two &&
	git checkout -b side HEAD^ &&
	test_commit three
'

test_expect_success 'reflog show works on branch (reflog not written for commits)' '
	git reflog show side >actual &&
	test_line_count -ge 1 actual
'

test_expect_success 'reflog walk with log -g (supported)' '
	git log -g --format="%gd %gs" >actual &&
	test_line_count -ge 3 actual
'

test_expect_success 'reflog walk with --walk-reflogs (supported)' '
	git log --walk-reflogs --format="%gd %gs" >actual &&
	test_line_count -ge 3 actual
'

test_done
