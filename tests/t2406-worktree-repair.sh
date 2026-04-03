#!/bin/sh

test_description='test git worktree remove and prune of stale worktrees'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit init
'

test_expect_success 'prune after removing worktree directory' '
	git worktree add --detach wt1 &&
	test_path_is_dir .git/worktrees/wt1 &&
	rm -rf wt1 &&
	git worktree prune &&
	test_path_is_missing .git/worktrees/wt1
'

test_expect_success 'remove worktree cleans up properly' '
	git worktree add --detach wt2 &&
	test_path_is_dir wt2 &&
	test_path_is_dir .git/worktrees/wt2 &&
	git worktree remove wt2 &&
	test_path_is_missing wt2 &&
	test_path_is_missing .git/worktrees/wt2
'

test_expect_success 'locked worktree not pruned' '
	git worktree add --detach wt3 &&
	git worktree lock wt3 &&
	rm -rf wt3 &&
	git worktree prune &&
	test_path_is_dir .git/worktrees/wt3 &&
	git worktree unlock wt3 &&
	git worktree prune &&
	test_path_is_missing .git/worktrees/wt3
'

test_done
