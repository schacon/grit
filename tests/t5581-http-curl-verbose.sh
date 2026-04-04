#!/bin/sh
#
# Upstream: t5581-http-curl-verbose.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test GIT_CURL_VERBOSE'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'failure in git-upload-pack is shown' '
	false
'

test_done
