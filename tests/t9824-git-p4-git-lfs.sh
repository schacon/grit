#!/bin/sh
#
# Upstream: t9824-git-p4-git-lfs.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='Clone repositories and store files in Git LFS'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
