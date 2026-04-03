#!/bin/sh

test_description='Test reflog interaction with detached HEAD'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not yet support log -g (reflog walk).
# We test checkout between branches and detached HEAD states,
# then mark reflog walk tests as expected failures.

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo initial >file &&
	git add file &&
	git commit -m initial &&
	git branch side &&
	echo second >file &&
	git add file &&
	git commit -m second
'

test_expect_success 'switch to branch' '
	git checkout side &&
	echo refs/heads/side >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'detach to other commit' '
	git checkout main &&
	MAIN_OID=$(git rev-parse main) &&
	git checkout "$MAIN_OID" &&
	test_must_fail git symbolic-ref HEAD
'

test_expect_success 'attach back to branch' '
	git checkout side &&
	echo refs/heads/side >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'baseline reflog walk' '
	git checkout main &&
	git log -g --format=%H >actual &&
	test_line_count -ge 2 actual &&
	head -1 actual >first &&
	git rev-parse main >expect &&
	test_cmp expect first
'

test_expect_success 'switch to branch reflog' '
	git checkout side &&
	git log -g --format=%H >actual &&
	head -1 actual >first &&
	SIDE_OID=$(git rev-parse side) &&
	echo "$SIDE_OID" >expect &&
	test_cmp expect first
'

test_done
