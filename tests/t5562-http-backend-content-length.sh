#!/bin/sh
#
# Upstream: t5562-http-backend-content-length.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test git-http-backend respects CONTENT_LENGTH (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not yet available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'fetch plain' '
	false
'

test_expect_failure 'fetch plain truncated' '
	false
'

test_expect_failure 'fetch plain empty' '
	false
'

test_expect_failure 'push plain' '
	false
'

test_expect_failure 'push plain truncated' '
	false
'

test_done
