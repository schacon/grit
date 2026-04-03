#!/bin/sh

test_description='stash with untracked files tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	mkdir repo &&
	cd repo &&
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo initial >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	git tag initial
'

test_expect_success 'stash with --include-untracked saves untracked files' '
	cd repo &&
	echo untracked >untracked-file &&
	git stash push --include-untracked &&
	test_path_is_missing untracked-file &&
	git stash pop &&
	test_path_is_file untracked-file &&
	rm untracked-file
'

test_expect_success 'stash push with -u is shorthand for --include-untracked' '
	cd repo &&
	echo untracked2 >untracked2 &&
	git stash push -u &&
	test_path_is_missing untracked2 &&
	git stash pop &&
	test_path_is_file untracked2 &&
	rm untracked2
'

test_expect_success 'stash with message' '
	cd repo &&
	echo change >file &&
	git stash push -m "my stash message" &&
	git stash list >actual &&
	grep "my stash message" actual &&
	git stash pop
'

test_expect_success 'stash branch creates branch from stash' '
	cd repo &&
	echo branched >file &&
	git stash &&
	git stash branch stash-branch &&
	echo branched >expect &&
	test_cmp expect file &&
	git checkout main &&
	git branch -d stash-branch
'

test_done
