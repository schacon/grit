#!/bin/sh
#
# Upstream: t9131-git-svn-empty-symlink.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='test that git handles an svn repository with empty symlinks'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
