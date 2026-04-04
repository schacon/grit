#!/bin/sh
#
# Upstream: t5557-http-get.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test downloading a file by URL'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

test_expect_failure 'get by URL: 404' '
	false
'

test_expect_failure 'get by URL: 200' '
	false
'

test_done
