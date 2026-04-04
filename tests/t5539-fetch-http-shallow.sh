#!/bin/sh
#
# Upstream: t5539-fetch-http-shallow.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='fetch/clone from a shallow clone over http'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

test_expect_failure 'setup shallow clone' '
	false
'

test_expect_failure 'clone http repository' '
	false
'

test_expect_failure 'no shallow lines after receiving ACK ready' '
	false
'

test_expect_failure 'clone shallow since ...' '
	false
'

test_expect_failure 'fetch shallow since ...' '
	false
'

test_expect_failure 'shallow clone exclude tag two' '
	false
'

test_expect_failure 'fetch exclude tag one' '
	false
'

test_expect_failure 'fetching deepen' '
	false
'

test_done
