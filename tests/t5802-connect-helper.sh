#!/bin/sh
# Ported from git/t/t5802-connect-helper.sh
# Tests for connect transport helper
#
# Requires git-remote-ext / connect transport. Stubbed.

test_description='connect transport helper'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'clone via ext:: transport' '
	test_create_repo server &&
	(cd server && test_commit one) &&
	git clone "ext::git %s server" client
'

test_expect_success 'fetch via ext:: transport' '
	(cd server && test_commit two) &&
	git -C client fetch "ext::git %s server"
'

test_done
