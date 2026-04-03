#!/bin/sh
# Adapted from git/t/t7003-filter-branch.sh
# Tests for git filter-branch (grit shows deprecation warning)

test_description='git filter-branch'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init filter-repo &&
	cd filter-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo A >a.t &&
	git add a.t &&
	git commit -m "A" &&
	git tag A &&

	echo B >b.t &&
	git add b.t &&
	git commit -m "B" &&
	git tag B &&

	echo C >c.t &&
	git add c.t &&
	git commit -m "C" &&
	git tag C
'

test_expect_success 'filter-branch shows deprecation warning' '
	cd filter-repo &&
	git filter-branch --msg-filter "cat" main >out 2>&1 || true &&
	# grit filter-branch shows a deprecation warning
	test_grep "deprecated" out || test_grep "filter-repo" out
'

test_expect_success 'commits are intact after filter-branch attempt' '
	cd filter-repo &&
	git log --oneline >log &&
	test_line_count = 3 log &&
	git rev-parse A &&
	git rev-parse B &&
	git rev-parse C
'

test_done
