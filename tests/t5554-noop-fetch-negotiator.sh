#!/bin/sh
#
# Upstream: t5554-noop-fetch-negotiator.sh
# Requires GIT_TRACE_PACKET support — stubbed as test_expect_failure.
# grit does not yet support fetch negotiation algorithm tracing.
#

test_description='test noop fetch negotiator'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- GIT_TRACE_PACKET not yet available in grit ---

test_expect_failure 'noop negotiator does not emit any "have"' '
	false
'

test_done
