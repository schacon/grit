#!/bin/sh

test_description='worktree add and list with multiple worktrees'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit first
'

test_expect_success 'worktree add creates separate working trees' '
	git worktree add wt1 &&
	git worktree add --detach wt2 main &&
	git worktree list >out &&
	test_line_count = 3 out
'

test_expect_success 'worktree list shows all worktrees' '
	git worktree list --porcelain >out &&
	grep "^worktree " out >actual &&
	test_line_count = 3 actual
'

test_expect_success 'worktree remove cleans up' '
	git worktree remove wt2 &&
	git worktree list >out &&
	test_line_count = 2 out
'

test_done
