#!/bin/sh

test_description='merge with unrelated index changes'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo base >file &&
	git add file &&
	git commit -m base &&
	git tag base &&

	git checkout -b side &&
	echo side >side-file &&
	git add side-file &&
	git commit -m "side adds side-file" &&

	git checkout main &&
	echo main >main-file &&
	git add main-file &&
	git commit -m "main adds main-file"
'

test_expect_success 'merge with clean index succeeds' '
	git merge side &&
	test_path_is_file file &&
	test_path_is_file side-file &&
	test_path_is_file main-file
'

test_expect_success 'setup for additional merge tests' '
	git checkout -b add-test base &&
	echo extra >extra-file &&
	git add extra-file &&
	git commit -m "add extra-file" &&
	git checkout -b add-side base &&
	echo other >other-file &&
	git add other-file &&
	git commit -m "add other-file"
'

test_expect_success 'merge with committed changes on both sides' '
	git checkout add-test &&
	git merge add-side &&
	test_path_is_file other-file &&
	test_path_is_file extra-file
'

test_expect_success 'fast-forward merge works' '
	git checkout base &&
	git merge side &&
	test_path_is_file side-file
'

test_expect_success 'merge --no-ff creates merge commit' '
	git checkout -b no-ff-test base &&
	git merge --no-ff side &&
	git log --oneline >log_out &&
	head -1 log_out | grep -i "merge"
'

test_done
