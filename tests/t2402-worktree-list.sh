#!/bin/sh

test_description='test git worktree list'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit init
'

test_expect_success '"list" all worktrees from main' '
	git worktree list >out &&
	test_line_count = 1 out &&
	test_grep "\\[main\\]" out
'

test_expect_success '"list" all worktrees --porcelain' '
	echo "worktree $(git rev-parse --show-toplevel)" >expect &&
	echo "HEAD $(git rev-parse HEAD)" >>expect &&
	echo "branch $(git symbolic-ref HEAD)" >>expect &&
	echo >>expect &&
	test_when_finished "rm -rf here && git worktree prune" &&
	git worktree add --detach here main &&
	echo "worktree $(pwd)/here" >>expect &&
	echo "HEAD $(git rev-parse HEAD)" >>expect &&
	echo "detached" >>expect &&
	echo >>expect &&
	git worktree list --porcelain >actual &&
	test_cmp expect actual
'

test_expect_success '"list" all worktrees with locked annotation' '
	test_when_finished "rm -rf locked unlocked && git worktree prune" &&
	git worktree add --detach locked main &&
	git worktree add --detach unlocked main &&
	git worktree lock locked &&
	test_when_finished "git worktree unlock locked" &&
	git worktree list >out &&
	grep "/locked  *[0-9a-f].* locked$" out &&
	! grep "/unlocked  *[0-9a-f].* locked$" out
'

test_expect_success '"list" all worktrees --porcelain with locked' '
	test_when_finished "rm -rf locked1 unlocked && git worktree prune" &&
	git worktree add --detach locked1 &&
	git worktree add --detach unlocked &&
	git worktree lock locked1 &&
	test_when_finished "git worktree unlock locked1" &&
	git worktree list --porcelain >out &&
	grep "^locked" out >actual &&
	echo "locked" >expect &&
	test_cmp expect actual
'

test_done
