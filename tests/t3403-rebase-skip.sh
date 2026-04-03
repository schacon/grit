#!/bin/sh
#
# Copyright (c) 2006 Eric Wong
#

test_description='git rebase --skip tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo hello >hello &&
	git add hello &&
	git commit -m "hello" &&
	git tag hello &&

	echo world >>hello &&
	git commit -a -m "hello world" &&
	echo goodbye >>hello &&
	git commit -a -m "goodbye" &&
	git tag goodbye
'

test_expect_success 'rebase --skip can not be used when no rebase in progress' '
	test_must_fail git rebase --skip
'

test_expect_success 'rebase --abort can not be used when no rebase in progress' '
	test_must_fail git rebase --abort
'

test_expect_success 'rebase --continue can not be used when no rebase in progress' '
	test_must_fail git rebase --continue
'

test_expect_success 'simple rebase succeeds' '
	git checkout -b side hello &&
	echo side >side-file &&
	git add side-file &&
	git commit -m "side" &&
	git rebase main &&
	test_path_is_file side-file &&
	test refs/heads/side = $(git symbolic-ref HEAD)
'

test_done
