#!/bin/sh
#
# Upstream: t9156-git-svn-fetch-deleted-tag-2.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn fetch deleted tag 2'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
