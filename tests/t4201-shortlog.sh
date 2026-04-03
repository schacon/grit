#!/bin/sh

test_description='git shortlog'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo 1 >a1 &&
	git add a1 &&
	test_tick &&
	git commit -m "first commit" &&
	echo 2 >a1 &&
	git add a1 &&
	test_tick &&
	git commit -m "second commit" &&
	echo 3 >a1 &&
	git add a1 &&
	test_tick &&
	git commit --author="Someone else <not@me>" -m "third commit"
'

test_expect_success 'shortlog groups by author' '
	git shortlog HEAD >actual &&
	grep "Test User" actual &&
	grep "Someone else" actual
'

test_expect_success 'shortlog -n sorts by count' '
	git shortlog -n HEAD >actual &&
	head -n1 actual | grep "Test User"
'

test_expect_success 'shortlog -s shows counts only' '
	git shortlog -s HEAD >actual &&
	grep "2" actual &&
	grep "1" actual
'

test_expect_success 'shortlog -e shows email' '
	git shortlog -e HEAD >actual &&
	grep "test@example.com" actual &&
	grep "not@me" actual
'

test_done
