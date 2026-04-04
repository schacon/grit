#!/bin/sh
#
# Upstream: t5552-skipping-fetch-negotiator.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test skipping fetch negotiator'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

test_expect_failure 'fetch.negotiationalgorithm config' '
	false
'

test_expect_failure 'commits with no parents are sent regardless of skip distance' '
	false
'

test_expect_failure 'when two skips collide, favor the larger one' '
	false
'

test_expect_failure 'use ref advertisement to filter out commits' '
	false
'

test_expect_failure 'handle clock skew' '
	false
'

test_expect_failure 'do not send "have" with ancestors of commits that server ACKed' '
	false
'

test_done
