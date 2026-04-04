#!/bin/sh
#
# Upstream: t5559-http-fetch-smart-http2.sh
# This is the HTTP/2 variant of t5551 — requires HTTP transport.
#

test_description='test smart fetching over HTTP (HTTP/2 variant)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP/2 transport not available in grit ---

test_expect_failure 'HTTP/2 smart fetch (variant of t5551)' '
	false
'

test_done
