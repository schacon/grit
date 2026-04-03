#!/bin/sh

test_description='merge rename directory scenarios'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	mkdir olddir &&
	echo file1 >olddir/file1 &&
	echo file2 >olddir/file2 &&
	git add olddir &&
	git commit -m base &&
	git tag base
'

test_expect_success 'one side renames dir files, other adds to old dir' '
	git checkout -b rename-side base &&
	mkdir newdir &&
	git mv olddir/file1 newdir/file1 &&
	git mv olddir/file2 newdir/file2 &&
	git commit -m "rename olddir->newdir" &&

	git checkout -b add-side base &&
	echo file3 >olddir/file3 &&
	git add olddir/file3 &&
	git commit -m "add to olddir" &&

	git checkout rename-side &&
	git merge add-side ||
	true
'

test_expect_success 'both sides add files in same new directory' '
	git checkout -b newdir1 base &&
	mkdir -p shared &&
	echo a >shared/a &&
	git add shared &&
	git commit -m "add shared/a" &&

	git checkout -b newdir2 base &&
	mkdir -p shared &&
	echo b >shared/b &&
	git add shared &&
	git commit -m "add shared/b" &&

	git checkout newdir1 &&
	git merge newdir2 &&
	test_path_is_file shared/a &&
	test_path_is_file shared/b
'

test_expect_success 'merge preserves nested directory structure' '
	git checkout -b nested1 base &&
	mkdir -p deep/nested/dir &&
	echo deep >deep/nested/dir/file &&
	git add deep &&
	git commit -m "add deep nested" &&

	git checkout -b nested2 base &&
	echo shallow >shallow &&
	git add shallow &&
	git commit -m "add shallow" &&

	git checkout nested1 &&
	git merge nested2 &&
	test_path_is_file deep/nested/dir/file &&
	test_path_is_file shallow
'

test_done
