#!/bin/sh
# Ported from git/t/t5801-remote-helpers.sh
# Tests for remote helper interface
#
# Requires remote-helper protocol support. Stubbed.

test_description='remote helpers'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'clone via remote helper' '
	test_create_repo server &&
	(cd server && test_commit one) &&
	git clone "testgit::${PWD}/server" client
'

test_expect_failure 'fetch via remote helper' '
	(cd server && test_commit two) &&
	git -C client fetch
'

test_expect_failure 'push via remote helper' '
	(cd client && test_commit three) &&
	git -C client push
'

test_done
