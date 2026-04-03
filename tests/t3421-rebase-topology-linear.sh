#!/bin/sh

test_description='git rebase topology tests (linear)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup linear history' '
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

	git checkout -b topic A &&
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

test_expect_success 'rebase linear topic onto main tip' '
	git checkout -b rebase1 T2 &&
	git rebase C &&
	echo C >expect &&
	test_cmp expect file &&
	test_path_is_file topic-file &&
	test_path_is_file topic-file2
'

test_expect_success 'rebase preserves linear history' '
	git log --oneline rebase1 >all &&
	git log --oneline C >base &&
	all_n=$(wc -l <all | tr -d " ") &&
	base_n=$(wc -l <base | tr -d " ") &&
	test $(( all_n - base_n )) = 2
'

test_expect_success 'rebase --onto moves commits to new base' '
	git checkout -b rebase2 T2 &&
	git rebase --onto B A &&
	echo B >expect &&
	test_cmp expect file &&
	test_path_is_file topic-file &&
	test_path_is_file topic-file2
'

test_expect_success 'rebase with same upstream is noop-like' '
	git checkout -b rebase3 T2 &&
	git rebase A &&
	test_path_is_file topic-file &&
	test_path_is_file topic-file2
'

test_expect_success 'rebase single commit' '
	git checkout -b rebase4 T1 &&
	git rebase C &&
	echo C >expect &&
	test_cmp expect file &&
	test_path_is_file topic-file
'

test_done
