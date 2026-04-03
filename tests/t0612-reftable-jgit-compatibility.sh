#!/bin/sh

test_description='reftables are compatible with JGit'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not have JGit integration.
# All JGit compatibility tests are expected failures.

test_expect_failure 'CGit repository can be read by JGit' '
	false
'

test_expect_failure 'JGit repository can be read by CGit' '
	false
'

test_done
