#!/bin/sh

test_description='test worktree HEAD management'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit init &&
	test_commit second
'

test_expect_success 'worktree add with branch creates branch' '
	git worktree add wt-branch -b new-branch &&
	test_path_is_dir .git/worktrees/wt-branch &&
	cat .git/worktrees/wt-branch/HEAD >actual &&
	echo "ref: refs/heads/new-branch" >expect &&
	test_cmp expect actual
'

test_expect_success 'worktree add --detach creates detached HEAD' '
	git worktree add --detach wt-detach &&
	cat .git/worktrees/wt-detach/HEAD >actual &&
	git rev-parse HEAD >expect &&
	test_cmp expect actual
'

test_expect_success 'each worktree has independent HEAD' '
	git worktree list --porcelain >out &&
	grep "^branch " out >branches &&
	grep "^detached" out >detached &&
	test_line_count -ge 1 branches &&
	test_line_count -ge 1 detached
'

test_done
