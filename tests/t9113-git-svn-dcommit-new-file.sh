#!/bin/sh
#
# Upstream: t9113-git-svn-dcommit-new-file.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn dcommit new files over svn:// test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
