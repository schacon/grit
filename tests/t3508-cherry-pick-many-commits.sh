#!/bin/sh
# Ported from git/t/t3508-cherry-pick-many-commits.sh
# Tests for cherry-picking many commits

test_description='test cherry-picking many commits'

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
	for val in second third fourth
	do
		echo $val >>file1 &&
		git add file1 &&
		test_tick &&
		git commit -m "$val" &&
		git tag $val || return 1
	done
'

test_expect_success 'cherry-pick first..fourth works' '
	git checkout -f main &&
	git reset --hard first &&
	test_tick &&
	git cherry-pick first..fourth &&
	git diff --quiet other &&
	git diff --quiet HEAD other
'

test_expect_success 'cherry-pick three one two works' '
	git checkout -f first &&
	test_commit one &&
	test_commit two &&
	test_commit three &&
	git checkout -f main &&
	git reset --hard first &&
	git cherry-pick three one two &&
	git diff --quiet three &&
	git diff --quiet HEAD three
'

test_expect_success 'revert fourth fourth~1 fourth~2 works' '
	git checkout -f main &&
	git reset --hard fourth &&
	test_tick &&
	git revert fourth fourth~1 fourth~2 &&
	git diff --quiet first &&
	git diff --cached --quiet first &&
	git diff --quiet HEAD first
'

test_done
