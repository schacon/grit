#!/bin/sh
#
# Upstream: t9831-git-p4-triggers.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 with server triggers'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
