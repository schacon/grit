#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5554-noop-fetch-negotiator.sh
# test noop fetch negotiator

test_description='test noop fetch negotiator'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
=======
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
>>>>>>> test/batch-EN
	false
'

test_done
