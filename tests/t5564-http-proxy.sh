#!/bin/sh
#
# Upstream: t5564-http-proxy.sh
# Requires HTTP proxy server — stubbed as test_expect_failure.
#

test_description='test fetching through http proxy (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP proxy transport not yet available in grit ---

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'proxy requires password' '
	false
'

test_expect_failure 'clone through proxy with auth' '
	false
'

test_expect_failure 'clone can prompt for proxy password' '
	false
'

test_done
