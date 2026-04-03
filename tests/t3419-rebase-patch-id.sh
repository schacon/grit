#!/bin/sh

test_description='git rebase - test patch id computation'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	test_tick &&
	git commit --allow-empty -m initial &&
	git tag root &&

	echo line1 >file &&
	git add file &&
	test_tick &&
	git commit -m "add file" &&
	git tag add-file &&

	git checkout -b other root &&
	echo other >other-file &&
	git add other-file &&
	test_tick &&
	git commit -m "add other" &&
	git tag add-other &&

	git cherry-pick add-file &&
	git tag cherry-picked
'

test_expect_success 'rebase with cherry-picked commit still works' '
	git checkout cherry-picked &&
	git rebase add-file &&
	test_path_is_file file &&
	test_path_is_file other-file
'

test_expect_success 'rebase with non-conflicting additional work' '
	git checkout -b extra add-other &&
	echo extra >extra-file &&
	git add extra-file &&
	test_tick &&
	git commit -m "extra work" &&
	git cherry-pick add-file &&
	git rebase add-file &&
	test_path_is_file other-file &&
	test_path_is_file extra-file &&
	test_path_is_file file
'

test_done
