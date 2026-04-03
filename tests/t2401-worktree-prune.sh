#!/bin/sh

test_description='prune $GIT_DIR/worktrees'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	git commit --allow-empty -m init
'

test_expect_success 'worktree prune on normal repo' '
	git worktree prune &&
	test_must_fail git worktree prune abc
'

test_expect_success 'prune directories without gitdir' '
	mkdir -p .git/worktrees/def/abc &&
	: >.git/worktrees/def/def &&
	git worktree prune --verbose 2>actual &&
	test_path_is_missing .git/worktrees/def
'

test_expect_success 'prune directories with invalid gitdir' '
	rm -rf .git/worktrees &&
	mkdir -p .git/worktrees/def/abc &&
	: >.git/worktrees/def/def &&
	: >.git/worktrees/def/gitdir &&
	git worktree prune --verbose 2>actual &&
	test_path_is_missing .git/worktrees/def
'

test_expect_success 'prune directories with gitdir pointing to nowhere' '
	rm -rf .git/worktrees &&
	mkdir -p .git/worktrees/def/abc &&
	: >.git/worktrees/def/def &&
	echo "$(pwd)"/nowhere >.git/worktrees/def/gitdir &&
	git worktree prune --verbose 2>actual &&
	test_path_is_missing .git/worktrees/def
'

test_expect_success 'not prune locked checkout' '
	test_when_finished "rm -rf .git/worktrees" &&
	mkdir -p .git/worktrees/ghi &&
	: >.git/worktrees/ghi/locked &&
	git worktree prune &&
	test_path_is_dir .git/worktrees/ghi
'

test_expect_success 'not prune proper checkouts' '
	test_when_finished "rm -rf .git/worktrees nop" &&
	git worktree add --detach "$PWD/nop" main &&
	git worktree prune &&
	test_path_is_dir .git/worktrees/nop
'

test_done
