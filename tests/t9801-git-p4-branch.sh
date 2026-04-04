#!/bin/sh
#
# Upstream: t9801-git-p4-branch.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 tests for p4 branches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
