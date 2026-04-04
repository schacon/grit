#!/bin/sh
#
# Upstream: t9168-git-svn-partially-globbed-names.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn globbing refspecs with prefixed globs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
