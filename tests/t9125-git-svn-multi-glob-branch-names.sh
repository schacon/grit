#!/bin/sh
#
# Upstream: t9125-git-svn-multi-glob-branch-names.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn multi-glob branch names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
