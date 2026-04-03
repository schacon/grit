#!/bin/sh

test_description='operations that cull histories in unusual ways'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_commit A &&
	test_commit B &&
	test_commit C &&
	git checkout -b side HEAD^ &&
	test_commit D &&
	test_commit E &&
	git merge -m Merged main
'

test_expect_success 'rev-list --first-parent --boundary' '
	cd repo &&
	git rev-list --first-parent --boundary HEAD^..HEAD
'

test_done
