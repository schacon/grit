#!/bin/sh
#
# Upstream: t9826-git-p4-keep-empty-commits.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='Clone repositories and keep empty commits'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
