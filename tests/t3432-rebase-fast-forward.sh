#!/bin/sh
# Ported from git/t/t3432-rebase-fast-forward.sh
# Tests for basic rebase fast-forward behavior

test_description='basic rebase fast-forward behavior'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	test_commit A &&
	test_commit B &&
	test_commit C &&
	test_commit D &&
	git checkout -b side D
'

test_expect_success 'rebase with no changes is a no-op' '
	git checkout side &&
	oldhead=$(git rev-parse HEAD) &&
	git rebase main &&
	newhead=$(git rev-parse HEAD) &&
	test "$oldhead" = "$newhead"
'

test_expect_success 'rebase --onto B B with no changes moves HEAD' '
	git checkout side &&
	git reset --hard D &&
	oldhead=$(git rev-parse HEAD) &&
	git rebase --onto B B &&
	newhead=$(git rev-parse HEAD) &&
	test "$oldhead" != "$newhead" &&
	git log --format=%s -n2 >actual &&
	test_write_lines D C >expect &&
	test_cmp expect actual
'

test_expect_success 'add work to side branch' '
	git checkout side &&
	git reset --hard D &&
	test_commit E
'

test_expect_success 'rebase our changes onto main succeeds' '
	git checkout side &&
	git reset --hard E &&
	git rebase main &&
	git log --format=%s >actual &&
	grep E actual &&
	grep D actual
'

test_done
