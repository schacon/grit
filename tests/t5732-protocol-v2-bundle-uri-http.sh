#!/bin/sh
#
# Upstream: t5732-protocol-v2-bundle-uri-http.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description="Test bundle-uri with protocol v2 and 'http://' transport (HTTP STUB)"

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport / bundle-uri not yet available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'clone with bundle-uri over http' '
	false
'

test_expect_failure 'fetch with bundle-uri over http' '
	false
'

test_done
