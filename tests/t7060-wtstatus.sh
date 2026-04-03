#!/bin/sh

test_description='basic work tree status reporting'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo initial >file &&
	git add file &&
	git commit -m initial &&
	git tag base
'

test_expect_success 'status on clean repo shows nothing tracked' '
	git status --porcelain >actual &&
	! grep "^[AMDRC]" actual
'

test_expect_success 'status shows staged new file' '
	echo new >staged &&
	git add staged &&
	git status --porcelain >actual &&
	grep "^A" actual | grep "staged"
'

test_expect_success 'status shows modified file' '
	git commit -m "add staged" &&
	echo modified >staged &&
	git status --porcelain >actual &&
	grep "M" actual | grep "staged"
'

test_expect_success 'status shows deleted file' '
	rm staged &&
	git status --porcelain >actual &&
	grep "D" actual | grep "staged"
'

test_expect_success 'status shows untracked files' '
	git checkout -- staged &&
	echo untracked >loose &&
	git status --porcelain >actual &&
	grep "^??" actual | grep "loose" &&
	rm loose
'

test_expect_success 'status --branch shows branch name' '
	git status --porcelain -b >actual &&
	grep "^## main" actual
'

test_expect_success 'status --branch on detached HEAD' '
	git checkout HEAD^0 &&
	git status --porcelain -b >actual &&
	grep "^## HEAD" actual &&
	git checkout main
'

test_expect_success 'status after merge shows clean' '
	git checkout -b side base &&
	echo side >side-file &&
	git add side-file &&
	git commit -m "side commit" &&
	git checkout main &&
	git merge side &&
	git status --porcelain >actual &&
	! grep "^[AMDRC]" actual
'

test_expect_success 'merge conflict creates MERGE_HEAD' '
	git checkout -b conflict-a base &&
	echo "from a" >file &&
	git add file &&
	git commit -m "a modifies file" &&

	git checkout -b conflict-b base &&
	echo "from b" >file &&
	git add file &&
	git commit -m "b modifies file" &&

	git checkout conflict-a &&
	test_must_fail git merge conflict-b &&
	test_path_is_file .git/MERGE_HEAD
'

test_expect_success 'merge --abort cleans up conflict' '
	git merge --abort &&
	test_path_is_missing .git/MERGE_HEAD
'

test_done
