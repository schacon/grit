#!/bin/sh
#
# Upstream: t9211-scalar-clone.sh
# Requires scalar — ported as test_expect_failure stubs.
#

test_description='test the `scalar clone` subcommand'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='scalar not available in grit'
test_done
