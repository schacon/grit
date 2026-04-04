#!/bin/sh
#
# Upstream: t9130-git-svn-authors-file.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn authors file tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
