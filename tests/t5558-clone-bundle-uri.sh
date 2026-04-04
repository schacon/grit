#!/bin/sh
#
# Upstream: t5558-clone-bundle-uri.sh
# Requires HTTP transport/bundle-uri — ported as test_expect_failure stubs.
#

test_description='test fetching bundles with --bundle-uri'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='HTTP transport/bundle-uri not available in grit'
test_done
