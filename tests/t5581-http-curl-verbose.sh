#!/bin/sh
#
# Upstream: t5581-http-curl-verbose.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test GIT_CURL_VERBOSE (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not yet available in grit ---

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'failure in git-upload-pack is shown' '
	false
'

test_done
