#!/bin/sh
#
# Upstream: t9134-git-svn-ignore-paths.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn property tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
