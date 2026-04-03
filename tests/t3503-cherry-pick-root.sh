#!/bin/sh

test_description='test cherry-picking (and reverting) a root commit'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo first >file1 &&
	git add file1 &&
	test_tick &&
	git commit -m "first" &&
	git tag first &&

	echo second >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m "second" &&
	git tag second
'

test_expect_success 'cherry-pick a non-root commit' '
	git checkout first &&
	git cherry-pick second &&
	test_path_is_file file2
'

test_expect_success 'revert a non-root commit' '
	git revert HEAD &&
	test_path_is_missing file2
'

test_expect_success 'cherry-pick with -x appends note' '
	git checkout first &&
	git cherry-pick -x second &&
	git log --format=%B --max-count=1 >msg &&
	grep "cherry picked from commit" msg
'

test_expect_success 'cherry-pick with --no-commit stages but does not commit' '
	git checkout first &&
	git cherry-pick --no-commit second &&
	test_path_is_file file2 &&
	git diff --cached --name-only >staged &&
	grep file2 staged &&
	git reset --hard
'

test_done
