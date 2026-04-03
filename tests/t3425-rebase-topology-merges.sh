#!/bin/sh
# Ported from git/t/t3425-rebase-topology-merges.sh
# Tests for rebase with different topology scenarios

test_description='git rebase topology tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup linear topology' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	test_commit A &&
	test_commit B &&
	test_commit C &&
	git checkout -b linear-topic A &&
	test_commit D &&
	test_commit E
'

test_expect_success 'rebase linear topic onto main' '
	git checkout linear-topic &&
	git rebase main &&
	git log --format=%s -n4 >actual &&
	test_write_lines E D C B >expect &&
	test_cmp expect actual
'

test_expect_success 'setup diverged topology' '
	git checkout main &&
	git checkout -b diverged A &&
	test_commit F &&
	git checkout main &&
	test_commit G
'

test_expect_success 'rebase diverged onto main' '
	git checkout diverged &&
	git rebase main &&
	git log --format=%s -n1 >actual &&
	echo F >expect &&
	test_cmp expect actual &&
	git merge-base --is-ancestor main HEAD
'

test_expect_success 'rebase --onto with three-arg form' '
	git checkout -b onto-test A &&
	test_commit H &&
	test_commit I &&
	git rebase --onto B A &&
	git log --format=%s -n2 >actual &&
	test_write_lines I H >expect &&
	test_cmp expect actual
'

test_done
