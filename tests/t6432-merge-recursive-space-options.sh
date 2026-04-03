#!/bin/sh

test_description='merge with space-related changes'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

# grit does not implement merge-recursive directly, but merges
# handle content conflicts. Test basic conflict detection with
# whitespace differences.

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	cat >file.txt <<-\EOF &&
	line one
	line two
	line three
	line four
	line five
	EOF
	git add file.txt &&
	git commit -m "Initial" &&
	git tag base
'

test_expect_success 'non-conflicting whitespace changes merge cleanly' '
	git checkout -b ws-main base &&
	cat >file.txt <<-\EOF &&
	line one modified
	line two
	line three
	line four
	line five
	EOF
	git add file.txt &&
	git commit -m "modify line one" &&

	git checkout -b ws-side base &&
	cat >file.txt <<-\EOF &&
	line one
	line two
	line three
	line four
	line five modified
	EOF
	git add file.txt &&
	git commit -m "modify line five" &&

	git checkout ws-main &&
	git merge ws-side &&
	grep "line one modified" file.txt &&
	grep "line five modified" file.txt
'

test_expect_success 'conflicting content changes detected' '
	git checkout -b conflict-a base &&
	cat >file.txt <<-\EOF &&
	line one from a
	line two
	line three
	line four
	line five
	EOF
	git add file.txt &&
	git commit -m "a modifies line one" &&

	git checkout -b conflict-b base &&
	cat >file.txt <<-\EOF &&
	line one from b
	line two
	line three
	line four
	line five
	EOF
	git add file.txt &&
	git commit -m "b modifies line one" &&

	git checkout conflict-a &&
	test_must_fail git merge conflict-b
'

test_expect_success 'conflict markers are present' '
	grep "<<<<<<" file.txt &&
	grep "======" file.txt &&
	grep ">>>>>>" file.txt
'

test_done
