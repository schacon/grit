#!/bin/sh

test_description='test show-branch'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo initial >file &&
	git add file &&
	git commit -m initial &&
	git tag initial &&
	git checkout -b branch1 initial &&
	echo branch1 >file &&
	git add file &&
	git commit -m branch1 &&
	git checkout -b branch2 initial &&
	echo branch2 >file &&
	git add file &&
	git commit -m branch2 &&
	git checkout -b branch3 initial &&
	echo branch3 >file &&
	git add file &&
	git commit -m branch3
'

test_expect_success 'show-branch with multiple branches' '
	git show-branch branch1 branch2 branch3 >actual &&
	grep "branch1" actual &&
	grep "branch2" actual &&
	grep "branch3" actual
'

test_done
