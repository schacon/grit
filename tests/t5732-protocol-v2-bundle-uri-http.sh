#!/bin/sh
#
# Upstream: t5732-protocol-v2-bundle-uri-http.sh
# Requires HTTP transport/bundle-uri — ported as test_expect_failure stubs.
#

test_description='Test bundle-uri with protocol v2 and 'http://' transport'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport/bundle-uri not available in grit ---

test_expect_failure 'HTTP transport/bundle-uri — t5732-protocol-v2-bundle-uri-http not yet ported' '
	false
'

test_done
