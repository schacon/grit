#!/bin/sh
#
# Upstream: t5812-proto-disable-http.sh
# Requires HTTP transport protocol — ported as test_expect_failure stubs.
#

test_description='test disabling of git-over-http in clone/fetch'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport protocol not available in grit ---

test_expect_failure 'create git-accessible repo' '
	false
'

test_expect_failure 'http(s) transport respects GIT_ALLOW_PROTOCOL' '
	false
'

test_expect_failure 'curl limits redirects' '
	false
'

test_expect_failure 'http can be limited to from-user' '
	false
'

test_done
