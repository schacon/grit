#!/bin/sh
#
# Upstream: t9808-git-p4-chdir.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 relative chdir'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
