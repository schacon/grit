#!/bin/sh

test_description='git rebase autostash tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo hello >file0 &&
	git add file0 &&
	test_tick &&
	git commit -m "initial commit" &&
	git tag initial &&

	git checkout -b feature-branch &&
	echo feature >file1 &&
	git add file1 &&
	test_tick &&
	git commit -m "feature commit" &&
	git tag feature &&

	git checkout main &&
	echo main-work >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m "main commit" &&
	git tag main-commit
'

test_expect_success 'stash before rebase, pop after' '
	git checkout feature-branch &&
	echo dirty >file0 &&
	git stash &&
	git rebase main &&
	git stash pop &&
	echo dirty >expect &&
	test_cmp expect file0 &&
	test_path_is_file file1 &&
	test_path_is_file file2
'

test_expect_success 'rebase with clean worktree succeeds' '
	git checkout -b clean-test feature &&
	git rebase main &&
	test_path_is_file file1 &&
	test_path_is_file file2
'

test_expect_success 'rebase onto is not confused by stash' '
	git checkout -b onto-test feature &&
	git reset --hard feature &&
	echo dirty >file0 &&
	git stash &&
	git rebase --onto main initial &&
	git stash pop &&
	echo dirty >expect &&
	test_cmp expect file0
'

test_done
