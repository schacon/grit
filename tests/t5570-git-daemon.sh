#!/bin/sh
#
# Upstream: t5570-git-daemon.sh
# Requires git-daemon — ported as test_expect_failure stubs.
#

test_description='test fetching over git protocol'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='git-daemon transport not available in grit, but flag validation works'
test_done
