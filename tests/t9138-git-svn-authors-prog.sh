#!/bin/sh
#
# Upstream: t9138-git-svn-authors-prog.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn authors prog tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
