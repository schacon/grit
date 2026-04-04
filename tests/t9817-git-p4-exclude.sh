#!/bin/sh
#
# Upstream: t9817-git-p4-exclude.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 tests for excluded paths during clone and sync'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
