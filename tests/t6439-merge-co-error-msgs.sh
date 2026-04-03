#!/bin/sh
# Adapted from git/t/t6439-merge-co-error-msgs.sh
# Tests error messages during merge

test_description='merge error messages'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init errmsg &&
	cd errmsg &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo base >shared &&
	git add shared &&
	git commit -m First &&
	git tag first &&

	git branch sideA &&
	git branch sideB &&

	git checkout sideA &&
	echo "version A" >shared &&
	git add shared &&
	git commit -m "A modifies shared" &&

	git checkout sideB &&
	echo "version B" >shared &&
	git add shared &&
	git commit -m "B modifies shared"
'

test_expect_success 'merge conflict produces error' '
	cd errmsg &&
	git checkout sideA &&
	test_must_fail git merge sideB -m "conflict" 2>err
'

test_expect_success 'merge --abort after conflict' '
	cd errmsg &&
	git merge --abort &&
	echo "version A" >expect &&
	test_cmp expect shared
'

test_done
