#!/bin/sh
#
# Upstream: t9823-git-p4-mock-lfs.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='Clone repositories and store files in Mock LFS'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
