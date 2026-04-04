#!/bin/sh
#
# Upstream: t9821-git-p4-path-variations.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='Clone repositories with path case variations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
