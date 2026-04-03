#!/bin/sh

test_description='merge with partial clone scenarios'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

# Partial clone requires server-side support not available in grit.
# Test basic merge scenarios that exercise the same code paths.

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo base >file &&
	mkdir dir &&
	echo base >dir/nested &&
	git add . &&
	git commit -m base &&
	git tag base
'

test_expect_success 'merge with new files on both sides' '
	git checkout -b side1 base &&
	echo side1 >new1 &&
	git add new1 &&
	git commit -m "add new1" &&

	git checkout -b side2 base &&
	echo side2 >new2 &&
	git add new2 &&
	git commit -m "add new2" &&

	git checkout side1 &&
	git merge side2 &&
	test_path_is_file new1 &&
	test_path_is_file new2
'

test_expect_success 'merge with nested directory changes' '
	git checkout -b nested1 base &&
	echo nested1 >dir/extra1 &&
	git add dir/extra1 &&
	git commit -m "add dir/extra1" &&

	git checkout -b nested2 base &&
	echo nested2 >dir/extra2 &&
	git add dir/extra2 &&
	git commit -m "add dir/extra2" &&

	git checkout nested1 &&
	git merge nested2 &&
	test_path_is_file dir/extra1 &&
	test_path_is_file dir/extra2 &&
	test_path_is_file dir/nested
'

test_done
