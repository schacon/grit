#!/bin/sh
# Ported from git/t/t3502-cherry-pick-merge.sh
# Tests for cherry-picking and reverting merges

test_description='cherry picking and reverting a merge'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	>A &&
	>B &&
	git add A B &&
	git commit -m "Initial" &&
	git tag initial &&
	git branch side &&
	echo new line >A &&
	git add A &&
	git commit -m "add line to A" &&
	git tag a &&
	git checkout side &&
	echo new line >B &&
	git add B &&
	git commit -m "add line to B" &&
	git tag b &&
	git checkout main &&
	git merge side &&
	git tag c
'

test_expect_success 'cherry-pick explicit first parent of a non-merge' '
	git reset --hard &&
	git checkout a^0 &&
	git cherry-pick -m 1 b &&
	git diff --exit-code c --
'

test_expect_success 'cherry pick a merge without -m should fail' '
	git reset --hard &&
	git checkout a^0 &&
	test_must_fail git cherry-pick c &&
	git diff --exit-code a --
'

test_expect_success 'cherry pick a merge (1)' '
	git reset --hard &&
	git checkout a^0 &&
	git cherry-pick -m 1 c &&
	git diff --exit-code c
'

test_expect_success 'cherry pick a merge (2)' '
	git reset --hard &&
	git checkout b^0 &&
	git cherry-pick -m 2 c &&
	git diff --exit-code c
'

test_expect_success 'cherry pick a merge relative to nonexistent parent should fail' '
	git reset --hard &&
	git checkout b^0 &&
	test_must_fail git cherry-pick -m 3 c
'

test_expect_success 'revert explicit first parent of a non-merge' '
	git reset --hard &&
	git checkout c^0 &&
	git revert -m 1 b &&
	git diff --exit-code a --
'

test_expect_success 'revert a merge without -m should fail' '
	git reset --hard &&
	git checkout c^0 &&
	test_must_fail git revert c &&
	git diff --exit-code c
'

test_expect_success 'revert a merge (1)' '
	git reset --hard &&
	git checkout c^0 &&
	git revert -m 1 c &&
	git diff --exit-code a --
'

test_expect_success 'revert a merge (2)' '
	git reset --hard &&
	git checkout c^0 &&
	git revert -m 2 c &&
	git diff --exit-code b --
'

test_expect_success 'revert a merge relative to nonexistent parent should fail' '
	git reset --hard &&
	git checkout c^0 &&
	test_must_fail git revert -m 3 c &&
	git diff --exit-code c
'

test_done
