#!/bin/sh

test_description='test git worktree lock and unlock'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit init &&
	git worktree add source
'

test_expect_success 'lock main worktree' '
	test_must_fail git worktree lock .
'

test_expect_success 'lock linked worktree' '
	git worktree lock source &&
	test_path_is_file .git/worktrees/source/locked
'

test_expect_success 'lock already locked worktree' '
	test_must_fail git worktree lock source
'

test_expect_success 'unlock linked worktree' '
	git worktree unlock source &&
	test_path_is_missing .git/worktrees/source/locked
'

test_expect_success 'unlock already unlocked worktree' '
	test_must_fail git worktree unlock source
'

test_expect_success 'remove locked worktree fails' '
	git worktree lock source &&
	test_must_fail git worktree remove source &&
	git worktree unlock source
'

test_expect_success 'remove linked worktree' '
	git worktree remove source &&
	test_path_is_missing source &&
	test_path_is_missing .git/worktrees/source
'

test_done
