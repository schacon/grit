#!/bin/sh

test_description='miscellaneous basic tests for cherry-pick and revert'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo initial >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	git tag initial &&

	echo added >>file &&
	git add file &&
	test_tick &&
	git commit -m added &&
	git tag added &&

	git checkout -b side initial &&
	echo side >side-file &&
	git add side-file &&
	test_tick &&
	git commit -m side &&
	git tag side-tag
'

test_expect_success 'cherry-pick --nonsense' '
	test_must_fail git cherry-pick --nonsense 2>msg
'

test_expect_success 'revert --nonsense' '
	test_must_fail git revert --nonsense 2>msg
'

test_expect_success 'cherry-pick a simple commit' '
	git checkout side &&
	git cherry-pick added &&
	grep added file
'

test_expect_success 'revert a simple commit' '
	git revert HEAD &&
	! grep added file
'

test_expect_success 'cherry-pick from another branch' '
	git checkout main &&
	git cherry-pick side-tag &&
	test_path_is_file side-file
'

test_expect_success 'cherry-pick multiple commits' '
	git checkout -b multi initial &&
	echo c1 >c1 &&
	git add c1 &&
	test_tick &&
	git commit -m c1 &&
	git tag c1 &&

	echo c2 >c2 &&
	git add c2 &&
	test_tick &&
	git commit -m c2 &&
	git tag c2 &&

	git checkout -b pick-multi initial &&
	git cherry-pick c1 c2 &&
	test_path_is_file c1 &&
	test_path_is_file c2
'

test_done
