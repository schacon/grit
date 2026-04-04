#!/bin/sh
#
# Upstream: t9122-git-svn-author.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn authorship'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
