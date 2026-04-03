#!/bin/sh

test_description='tests for git history (log) with branches'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m "base" &&
	git tag base &&

	git checkout -b branch1 &&
	echo b1 >b1-file &&
	git add b1-file &&
	test_tick &&
	git commit -m "branch1 commit" &&
	git tag b1 &&

	git checkout main &&
	git checkout -b branch2 &&
	echo b2 >b2-file &&
	git add b2-file &&
	test_tick &&
	git commit -m "branch2 commit" &&
	git tag b2
'

test_expect_success 'history shows branch-specific commits' '
	git checkout branch1 &&
	git history --oneline >actual &&
	test_line_count = 2 actual
'

test_expect_success 'history with --all shows all branches' '
	git history --oneline --all >actual &&
	test_line_count -ge 3 actual
'

test_expect_success 'history respects branch filter' '
	git history --oneline branch1 >actual &&
	test_line_count = 2 actual
'

test_expect_success 'history range between branches' '
	git history --oneline base..branch1 >actual &&
	test_line_count = 1 actual
'

test_done
