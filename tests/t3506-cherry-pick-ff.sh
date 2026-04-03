#!/bin/sh
# Ported from git/t/t3506-cherry-pick-ff.sh
# Cherry-pick basic tests (--ff not supported, testing core cherry-pick)

test_description='test cherry-pick basic operations'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo first >file1 &&
	git add file1 &&
	test_tick &&
	git commit -m "first" &&
	git tag first &&

	git checkout -b other &&
	echo second >>file1 &&
	git add file1 &&
	test_tick &&
	git commit -m "second" &&
	git tag second
'

test_expect_success 'cherry-pick creates new commit with different hash' '
	git checkout main &&
	git reset --hard first &&
	test_tick &&
	git cherry-pick second &&
	test "$(git rev-parse HEAD)" != "$(git rev-parse second)"
'

test_expect_success 'cherry-pick applies the correct diff' '
	git checkout main &&
	git reset --hard first &&
	git cherry-pick second &&
	git diff --quiet second --
'

test_expect_success 'cherry-pick preserves commit message' '
	git checkout main &&
	git reset --hard first &&
	git cherry-pick second &&
	git log --format=%s -n1 >actual &&
	echo "second" >expect &&
	test_cmp expect actual
'

test_expect_success 'cherry-pick merge without -m should fail' '
	git checkout main &&
	git reset --hard first &&
	echo new line >A &&
	git add A &&
	test_tick &&
	git commit -m "add line to A" &&
	git tag a &&
	git checkout -b side2 first &&
	echo new line >B &&
	git add B &&
	test_tick &&
	git commit -m "add line to B" &&
	git tag b &&
	git checkout main &&
	git merge side2 &&
	git tag c &&
	git checkout -b new a &&
	test_must_fail git cherry-pick c
'

test_done
