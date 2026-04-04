#!/bin/sh
#
# Upstream: t9151-svn-mergeinfo.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git-svn svn mergeinfo properties'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
