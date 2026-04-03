#!/bin/sh

test_description='git rebase -i basic tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	echo a >file &&
	git add file &&
	test_tick &&
	git commit -m "a" &&

	echo b >>file &&
	git add file &&
	test_tick &&
	git commit -m "b" &&

	echo c >>file &&
	git add file &&
	test_tick &&
	git commit -m "c" &&

	git checkout -b topic HEAD~2 &&
	echo d >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m "d"
'

test_expect_success 'rebase -i shows pick lines' '
	git checkout topic &&
	EDITOR=cat git rebase -i main >todo 2>&1 || true &&
	grep "pick" todo
'

test_expect_success 'rebase -i todo contains the commit subject' '
	grep "d" todo
'

test_done
