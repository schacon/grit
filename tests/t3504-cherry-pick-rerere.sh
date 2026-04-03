#!/bin/sh

test_description='cherry-pick should handle rerere for conflicts'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo initial >foo &&
	git add foo &&
	test_tick &&
	git commit -m initial &&
	git tag initial &&

	echo main-change >foo &&
	git add foo &&
	test_tick &&
	git commit -m foo-main &&
	git tag foo-main &&

	git checkout -b dev initial &&
	echo dev-change >foo &&
	git add foo &&
	test_tick &&
	git commit -m foo-dev &&
	git tag foo-dev
'

test_expect_success 'cherry-pick detects conflict' '
	git checkout dev &&
	test_must_fail git cherry-pick foo-main
'

test_expect_success 'cherry-pick --abort after conflict' '
	git cherry-pick --abort &&
	echo dev-change >expect &&
	test_cmp expect foo
'

test_expect_success 'cherry-pick with -n applies without committing' '
	git checkout initial &&
	git cherry-pick -n foo-main &&
	git diff --cached --name-only >staged &&
	grep foo staged &&
	git reset --hard
'

test_expect_success 'cherry-pick with --signoff adds trailer' '
	git checkout -b signoff-test initial &&
	echo new-content >newfile &&
	git add newfile &&
	test_tick &&
	git commit -m "new file" &&
	git tag new-file &&

	git checkout -b signoff-branch initial &&
	git cherry-pick --signoff new-file &&
	git log --format=%B --max-count=1 >msg &&
	grep "Signed-off-by:" msg
'

test_done
