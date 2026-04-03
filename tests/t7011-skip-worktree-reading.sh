#!/bin/sh

test_description='skip-worktree bit test'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo initial >file &&
	git add file &&
	git commit -m initial &&
	mkdir sub &&
	echo 1 >1 &&
	echo 2 >2 &&
	echo sub1 >sub/1 &&
	echo sub2 >sub/2 &&
	git add 1 2 sub/1 sub/2 &&
	git commit -m "add numbered files"
'

test_expect_success 'update-index --skip-worktree marks file' '
	git update-index --skip-worktree 1 &&
	git ls-files --stage 1 >actual &&
	grep "1$" actual
'

test_expect_success 'update-index --no-skip-worktree unmarks file' '
	git update-index --no-skip-worktree 1 &&
	git ls-files --stage 1 >actual &&
	grep "1$" actual
'

test_expect_success 'skip-worktree file absent from ls-files --modified' '
	git update-index --skip-worktree 1 &&
	rm 1 &&
	git ls-files -m >actual &&
	! grep "^1$" actual
'

test_expect_success 'skip-worktree file absent from ls-files --deleted' '
	git ls-files -d >actual &&
	! grep "^1$" actual
'

test_expect_success 'update-index --remove on skip-worktree file' '
	git update-index --remove 1 &&
	git ls-files 1 >actual &&
	test_must_be_empty actual
'

test_expect_success 'restore and re-add for further tests' '
	echo 1 >1 &&
	git add 1 &&
	git update-index --skip-worktree sub/1 &&
	rm sub/1 &&
	git ls-files -m >actual &&
	! grep "sub/1" actual
'

test_done
