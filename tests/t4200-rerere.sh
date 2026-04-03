#!/bin/sh
#
# Copyright (c) 2006 Johannes E. Schindelin
#

test_description='git rerere basic operations'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&

	echo "base content" >file &&
	git add file &&
	test_tick &&
	git commit -q -m initial
'

test_expect_success 'rerere status with no conflicts' '
	git rerere status >output 2>&1 &&
	true
'

test_expect_success 'rerere diff with no conflicts' '
	git rerere diff >output 2>&1 &&
	true
'

test_expect_success 'rerere clear runs without error' '
	git rerere clear 2>&1 &&
	true
'

test_expect_success 'rerere gc runs without error' '
	git rerere gc 2>&1 &&
	true
'

test_expect_success 'rerere forget requires pathspec' '
	test_must_fail git rerere forget
'

test_done
