#!/bin/sh

test_description='merge conflicts with sparse checkout'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo base >file &&
	echo sparse-file >sparse &&
	git add file sparse &&
	git commit -m base &&
	git tag base
'

test_expect_success 'enable sparse checkout and verify cone' '
	git sparse-checkout set file &&
	test_path_is_file file
'

test_expect_success 'merge works with sparse checkout active' '
	git checkout -b side base &&
	git sparse-checkout disable &&
	echo side >side-file &&
	git add side-file &&
	git commit -m "add side-file" &&

	git checkout main &&
	git sparse-checkout set file side-file &&
	git merge side &&
	test_path_is_file side-file
'

test_done
