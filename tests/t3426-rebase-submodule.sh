#!/bin/sh

test_description='rebase with non-conflicting branches'

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

	git checkout -b feature &&
	echo feat1 >feat1 &&
	git add feat1 &&
	test_tick &&
	git commit -m feat1 &&

	echo feat2 >feat2 &&
	git add feat2 &&
	test_tick &&
	git commit -m feat2 &&
	git tag feature-end &&

	git checkout main &&
	echo main1 >main1 &&
	git add main1 &&
	test_tick &&
	git commit -m main1 &&
	git tag main-end
'

test_expect_success 'rebase feature onto main' '
	git checkout feature &&
	git rebase main &&
	test_path_is_file feat1 &&
	test_path_is_file feat2 &&
	test_path_is_file main1
'

test_expect_success 'rebase --onto moves subset of commits' '
	git checkout -b subset feature-end &&
	git rebase --onto main-end base &&
	test_path_is_file feat1 &&
	test_path_is_file feat2 &&
	test_path_is_file main1
'

test_done
