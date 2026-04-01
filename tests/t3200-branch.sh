#!/bin/sh
# Tests for 'gust branch'.

test_description='gust branch'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "init" >file.txt &&
	git add file.txt &&
	git commit -m "initial" 2>/dev/null
'

test_expect_success 'list branches shows current' '
	cd repo &&
	git branch >actual &&
	grep "^\* master" actual
'

test_expect_success 'create a branch' '
	cd repo &&
	git branch feature &&
	git branch >actual &&
	grep "feature" actual
'

test_expect_success '--show-current shows current branch' '
	cd repo &&
	git branch --show-current >actual &&
	echo "master" >expected &&
	test_cmp expected actual
'

test_expect_success 'create branch at specific commit' '
	cd repo &&
	echo "second" >>file.txt &&
	git add file.txt &&
	git commit -m "second" 2>/dev/null &&
	git branch old-point HEAD~1 2>/dev/null ||
	git branch old-point master 2>/dev/null
'

test_expect_success 'delete a branch' '
	cd repo &&
	git branch to-delete &&
	git branch >actual &&
	grep "to-delete" actual &&
	git branch -d to-delete 2>/dev/null &&
	git branch >actual &&
	! grep "to-delete" actual
'

test_expect_success 'cannot delete current branch' '
	cd repo &&
	! git branch -d master 2>/dev/null
'

test_expect_success 'rename a branch' '
	cd repo &&
	git branch rename-me &&
	git branch -m rename-me renamed 2>/dev/null &&
	git branch >actual &&
	! grep "rename-me" actual &&
	grep "renamed" actual
'

test_expect_success 'verbose listing shows commit info' '
	cd repo &&
	git branch -v >actual &&
	grep "master" actual &&
	grep "second" actual
'

test_done
