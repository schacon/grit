#!/bin/sh
# Ported from git/t/t3427-rebase-subtree.sh
# Basic rebase --onto tests

test_description='git rebase --onto tests'

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
	git checkout -b topic B &&
	test_commit D &&
	test_commit E
'

test_expect_success 'rebase --onto main topic rebases correctly' '
	git checkout topic &&
	git rebase --onto main B &&
	git log --format=%s -n2 >actual &&
	test_write_lines E D >expect &&
	test_cmp expect actual &&
	git merge-base --is-ancestor main HEAD
'

test_expect_success 'rebase --onto specific commit' '
	git checkout -b topic2 B &&
	test_commit F &&
	git rebase --onto A B &&
	git log --format=%s -n1 >actual &&
	echo F >expect &&
	test_cmp expect actual &&
	parent=$(git rev-parse HEAD^) &&
	a_commit=$(git rev-parse A) &&
	test "$parent" = "$a_commit"
'

test_done
