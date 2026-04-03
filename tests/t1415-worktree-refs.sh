#!/bin/sh

test_description='per-worktree refs'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "t@t" &&
	test_commit initial &&
	test_commit second &&
	git worktree add wt1 -b wt1-branch &&
	(cd wt1 && test_commit wt1-commit)
'

test_expect_success 'refs/worktree are per-worktree' '
	git update-ref refs/worktree/foo HEAD &&
	git -C wt1 update-ref refs/worktree/foo HEAD &&
	main_foo=$(git rev-parse refs/worktree/foo) &&
	wt1_foo=$(git -C wt1 rev-parse refs/worktree/foo) &&
	test "$main_foo" != "$wt1_foo"
'

test_expect_success 'for-each-ref shows worktree refs' '
	git for-each-ref --format="%(refname)" refs/worktree/ >actual &&
	echo refs/worktree/foo >expected &&
	test_cmp expected actual
'

test_expect_success 'for-each-ref from linked worktree shows its own worktree refs' '
	git -C wt1 for-each-ref --format="%(refname)" refs/worktree/ >actual &&
	echo refs/worktree/foo >expected &&
	test_cmp expected actual
'

test_done
