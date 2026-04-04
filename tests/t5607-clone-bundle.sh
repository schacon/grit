#!/bin/sh

test_description='some bundle related tests'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit initial &&
	test_tick &&
	git tag -m tag tag &&
	test_commit second &&
	test_commit third &&
	git tag -d initial &&
	git tag -d second &&
	git tag -d third
'

test_expect_success 'create bundle' '
	git bundle create tip.bundle -1 main
'

test_expect_success 'verify bundle' '
	git bundle verify tip.bundle
'

test_expect_success 'ls-remote bundle' '
	git ls-remote tip.bundle >actual &&
	test -s actual
'

test_expect_success 'clone from bundle' '
	git clone tip.bundle cloned &&
	test_path_is_dir cloned
'

test_done
