#!/bin/sh
#
# Upstream: t5549-fetch-push-http.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='fetch/push functionality using the HTTP protocol'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

test_expect_failure 'push without negotiation (for comparing object counts with the next test)' '
	false
'

test_expect_failure 'push with negotiation' '
	false
'

test_expect_failure 'push with negotiation proceeds anyway even if negotiation fails' '
	false
'

test_done
