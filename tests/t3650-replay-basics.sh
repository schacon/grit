#!/bin/sh

test_description='basic rebase and cherry-pick replay tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo A >file &&
	git add file &&
	test_tick &&
	git commit -m A &&
	git tag A &&

	echo B >file &&
	git add file &&
	test_tick &&
	git commit -m B &&
	git tag B &&

	git checkout -b topic1 A &&
	echo C >topic-file &&
	git add topic-file &&
	test_tick &&
	git commit -m C &&
	git tag C &&

	echo D >topic-file2 &&
	git add topic-file2 &&
	test_tick &&
	git commit -m D &&
	git tag D
'

test_expect_success 'cherry-pick replays single commit' '
	git checkout B &&
	git cherry-pick C &&
	test_path_is_file topic-file
'

test_expect_success 'cherry-pick replays multiple commits' '
	git checkout B &&
	git cherry-pick C D &&
	test_path_is_file topic-file &&
	test_path_is_file topic-file2
'

test_expect_success 'rebase replays topic onto main' '
	git checkout -b replay-test D &&
	git rebase B &&
	test_path_is_file topic-file &&
	test_path_is_file topic-file2 &&
	echo B >expect &&
	test_cmp expect file
'

test_expect_success 'cherry-pick with -x adds provenance' '
	git checkout B &&
	git cherry-pick -x C &&
	git log --format=%B --max-count=1 >msg &&
	grep "cherry picked from commit" msg
'

test_done
