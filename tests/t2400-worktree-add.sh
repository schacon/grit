#!/bin/sh

test_description='test git worktree add'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit init
'

test_expect_success '"add" an existing empty worktree' '
	mkdir existing_empty &&
	git worktree add --detach existing_empty main
'

test_expect_success '"add" worktree creates gitdir' '
	git worktree add --detach here main &&
	test_path_is_file here/.git &&
	test_path_is_dir .git/worktrees/here
'

test_expect_success '"add" worktree sets HEAD' '
	git rev-parse HEAD >expect &&
	git -C here rev-parse HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"add" worktree .git file points to correct gitdir' '
	echo "gitdir: $(pwd)/.git/worktrees/here" >expect &&
	test_cmp expect here/.git
'

test_expect_success '"add" from a linked checkout' '
	(
		cd here &&
		git worktree add --detach nested main
	) &&
	test_path_is_dir .git/worktrees/nested
'

test_expect_success '"list" shows main and linked worktrees' '
	git worktree list --porcelain >out &&
	grep "^worktree " out >actual &&
	test_line_count -ge 2 actual
'

test_expect_success '"add" worktree with detached HEAD' '
	git worktree add --detach detached main &&
	git -C detached rev-parse HEAD >actual &&
	git rev-parse HEAD >expect &&
	test_cmp expect actual &&
	test_must_fail git -C detached symbolic-ref HEAD
'

test_done
