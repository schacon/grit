#!/bin/sh
# Ported from git/t/t3431-rebase-fork-point.sh
# Basic rebase tests

test_description='git rebase basic fork-point scenarios'

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
	git checkout -b side B &&
	test_commit D &&
	test_commit E
'

test_expect_success 'rebase side onto main' '
	git checkout side &&
	git rebase main &&
	git log --format=%s -n4 >actual &&
	test_write_lines E D C B >expect &&
	test_cmp expect actual
'

test_expect_success 'rebase --onto specific base' '
	git checkout -b side2 B &&
	test_commit F &&
	git rebase --onto A B &&
	git log --format=%s -n1 >actual &&
	echo F >expect &&
	test_cmp expect actual
'

test_expect_success 'rebase with no changes needed is a noop' '
	git checkout main &&
	old=$(git rev-parse HEAD) &&
	git rebase main &&
	new=$(git rev-parse HEAD) &&
	test "$old" = "$new"
'

test_done
