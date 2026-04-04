#!/bin/sh
#
# Upstream: t5580-unc-paths.sh
# Windows-only UNC path tests — skip on non-Windows.
#

test_description='various Windows-only path tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# This test is Windows-only (requires CYGWIN or MINGW).
# On Linux, skip all tests.
skip_all='skipping Windows-only UNC path tests'
test_done
