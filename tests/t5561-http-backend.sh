#!/bin/sh
#
# Upstream: t5561-http-backend.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test git-http-backend (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not yet available in grit ---

test_expect_failure 'server request log matches test results' '
	false
'

test_done
