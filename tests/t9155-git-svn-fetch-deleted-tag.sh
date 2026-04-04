#!/bin/sh
#
# Upstream: t9155-git-svn-fetch-deleted-tag.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn fetch deleted tag'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
