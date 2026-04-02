#!/bin/sh

test_description='Test reflog exists'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	test_commit A
'

test_expect_success 'reflog exists works' '
	git reflog exists refs/heads/main &&
	test_must_fail git reflog exists refs/heads/nonexistent
'

test_done
