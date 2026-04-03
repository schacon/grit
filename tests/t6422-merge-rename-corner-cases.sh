#!/bin/sh

test_description='merge rename corner cases'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup base' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo base >file &&
	git add file &&
	git commit -m base &&
	git tag base
'

test_expect_success 'rename on one side, modify on other' '
	git checkout -b rename1 base &&
	git mv file renamed &&
	git commit -m "rename file->renamed" &&

	git checkout -b modify1 base &&
	echo extra >> file &&
	git add file &&
	git commit -m "modify file" &&

	git checkout rename1 &&
	git merge modify1 ||
	true
'

test_expect_success 'both sides add same filename with same content' '
	git reset --hard rename1 &&
	git checkout -b add-same1 base &&
	echo same >newfile &&
	git add newfile &&
	git commit -m "add newfile on branch 1" &&

	git checkout -b add-same2 base &&
	echo same >newfile &&
	git add newfile &&
	git commit -m "add newfile on branch 2" &&

	git checkout add-same1 &&
	git merge add-same2 &&
	echo same >expect &&
	test_cmp expect newfile
'

test_expect_success 'merge with file removed on one side, new file on other' '
	git checkout -b rm1 base &&
	git rm file &&
	git commit -m "remove file" &&

	git checkout -b keep1 base &&
	echo extra >extra &&
	git add extra &&
	git commit -m "add extra" &&

	git checkout rm1 &&
	git merge keep1 &&
	test_path_is_file extra
'

test_expect_success 'both sides modify different files' '
	git checkout -b mod-a base &&
	echo a >a &&
	git add a &&
	git commit -m "add a" &&

	git checkout -b mod-b base &&
	echo b >b &&
	git add b &&
	git commit -m "add b" &&

	git checkout mod-a &&
	git merge mod-b &&
	test_path_is_file a &&
	test_path_is_file b
'

test_done
