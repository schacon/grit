#!/bin/sh
#
# Upstream: t5560-http-backend-noserver.sh
# Tests git http-backend without a server (direct CGI invocation).
# grit has a stub http-backend that returns 501 — stubbed as test_expect_failure.
#

test_description='test git-http-backend-noserver'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- http-backend not yet implemented in grit ---

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'http-backend blocks bad PATH_INFO' '
	false
'

test_done
