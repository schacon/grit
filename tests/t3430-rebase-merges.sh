#!/bin/sh
# Ported from git/t/t3430-rebase-merges.sh
# Basic rebase tests with merge scenarios

test_description='git rebase with merges'

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
	git checkout -b topic A &&
	test_commit D &&
	test_commit E
'

test_expect_success 'rebase topic onto main' '
	git checkout topic &&
	git rebase main &&
	git log --format=%s -n2 >actual &&
	test_write_lines E D >expect &&
	test_cmp expect actual &&
	git merge-base --is-ancestor main HEAD
'

test_expect_success 'rebase preserves commit messages' '
	git log --format=%s -n1 >actual &&
	echo E >expect &&
	test_cmp expect actual
'

test_expect_success 'rebase --onto with different base' '
	git checkout -b topic2 A &&
	test_commit F &&
	test_commit G &&
	git rebase --onto B A &&
	git log --format=%s -n2 >actual &&
	test_write_lines G F >expect &&
	test_cmp expect actual &&
	parent_of_f=$(git rev-parse HEAD~1^) &&
	b_commit=$(git rev-parse B) &&
	test "$parent_of_f" = "$b_commit"
'

test_done
