#!/bin/sh
#
# Upstream: t5541-http-push-smart.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test smart pushing over http via http-backend'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='HTTP transport not available in grit'
test_done
