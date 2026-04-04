#!/bin/sh
#
# Upstream: t9146-git-svn-empty-dirs.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn creates empty directories'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
