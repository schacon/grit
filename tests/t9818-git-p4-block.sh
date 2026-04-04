#!/bin/sh
#
# Upstream: t9818-git-p4-block.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 fetching changes in multiple blocks'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
