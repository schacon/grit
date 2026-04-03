#!/bin/sh

test_description='git rebase --continue tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m base &&
	git tag base &&

	git checkout -b topic &&
	echo topic1 >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m topic1 &&

	echo topic2 >file3 &&
	git add file3 &&
	test_tick &&
	git commit -m topic2 &&

	git checkout main &&
	echo main-change >file4 &&
	git add file4 &&
	test_tick &&
	git commit -m main-change
'

test_expect_success 'rebase --continue without rebase in progress fails' '
	test_must_fail git rebase --continue
'

test_expect_success 'rebase --abort without rebase in progress fails' '
	test_must_fail git rebase --abort
'

test_expect_success 'non-conflicting rebase does not need --continue' '
	git checkout topic &&
	git rebase main &&
	test_path_is_file file2 &&
	test_path_is_file file3 &&
	test_path_is_file file4
'

test_expect_success 'setup for conflict test' '
	git checkout -b conflict-base base &&
	echo conflict-A >conflict-file &&
	git add conflict-file &&
	test_tick &&
	git commit -m conflict-A &&
	git tag conflict-A &&

	git checkout -b conflict-B base &&
	echo conflict-B >conflict-file &&
	git add conflict-file &&
	test_tick &&
	git commit -m conflict-B
'

test_expect_success 'rebase with conflict can be aborted' '
	git checkout conflict-B &&
	test_must_fail git rebase conflict-A &&
	git rebase --abort &&
	echo conflict-B >expect &&
	test_cmp expect conflict-file
'

test_expect_success 'rebase with conflict can be skipped' '
	git checkout conflict-B &&
	test_must_fail git rebase conflict-A &&
	git rebase --skip &&
	echo conflict-A >expect &&
	test_cmp expect conflict-file
'

test_done
