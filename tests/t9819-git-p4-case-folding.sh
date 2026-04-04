#!/bin/sh
#
# Upstream: t9819-git-p4-case-folding.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='interaction with P4 case-folding'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
