#!/bin/sh
#
# Upstream: t5542-push-http-shallow.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='push from/to a shallow clone over http'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'push to shallow repo via http' '
	false
'

test_expect_failure 'push from shallow repo via http' '
	false
'

test_done
