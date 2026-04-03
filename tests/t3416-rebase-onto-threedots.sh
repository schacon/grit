#!/bin/sh

test_description='git rebase --onto tests'

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

	echo C >file &&
	git add file &&
	test_tick &&
	git commit -m C &&
	git tag C &&

	git checkout -b topic B &&
	echo T1 >topic-file &&
	git add topic-file &&
	test_tick &&
	git commit -m T1 &&
	git tag T1 &&

	echo T2 >topic-file2 &&
	git add topic-file2 &&
	test_tick &&
	git commit -m T2 &&
	git tag T2
'

test_expect_success 'rebase --onto B topic onto C' '
	git checkout -b test1 T2 &&
	git rebase --onto C B &&
	echo C >expect &&
	test_cmp expect file &&
	test_path_is_file topic-file &&
	test_path_is_file topic-file2
'

test_expect_success 'rebase --onto with explicit upstream' '
	git checkout -b test2 T2 &&
	git rebase --onto A B &&
	echo A >expect &&
	test_cmp expect file &&
	test_path_is_file topic-file &&
	test_path_is_file topic-file2
'

test_expect_success 'rebase --onto with same base is no-op-like' '
	git checkout -b test3 T2 &&
	git rebase --onto B B &&
	test_path_is_file topic-file &&
	test_path_is_file topic-file2
'

test_done
