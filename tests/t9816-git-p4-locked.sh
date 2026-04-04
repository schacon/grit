#!/bin/sh
#
# Upstream: t9816-git-p4-locked.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 locked file behavior'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
