#!/bin/sh

test_description='recursive merge corner cases'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

# These tests exercise merge behavior in corner cases.
# grit does not implement -s recursive, so we test basic merge behavior.

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo base >file &&
	git add file &&
	git commit -m base &&
	git tag base
'

test_expect_success 'simple content conflict detected' '
	git checkout -b left base &&
	echo left >file &&
	git add file &&
	git commit -m left &&

	git checkout -b right base &&
	echo right >file &&
	git add file &&
	git commit -m right &&

	git checkout left &&
	test_must_fail git merge right
'

test_expect_success 'conflict markers present' '
	grep "<<<<<<" file &&
	grep "======" file &&
	grep ">>>>>>" file
'

test_expect_success 'merge --abort cleans up' '
	git merge --abort &&
	echo left >expect &&
	test_cmp expect file
'

test_expect_success 'non-conflicting merge succeeds' '
	git checkout -b nc-left base &&
	echo left >left-file &&
	git add left-file &&
	git commit -m "add left-file" &&

	git checkout -b nc-right base &&
	echo right >right-file &&
	git add right-file &&
	git commit -m "add right-file" &&

	git checkout nc-left &&
	git merge nc-right &&
	test_path_is_file left-file &&
	test_path_is_file right-file
'

test_expect_success 'criss-cross merge base' '
	git checkout -b criss base &&
	echo criss >file &&
	git add file &&
	git commit -m criss &&

	git checkout -b cross base &&
	echo cross >file2 &&
	git add file2 &&
	git commit -m cross &&

	git checkout criss &&
	git merge cross -m "criss merges cross" &&
	test_path_is_file file &&
	test_path_is_file file2
'

test_done
