#!/bin/sh
#
# Upstream: t9115-git-svn-dcommit-funky-renames.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn dcommit can commit renames of files with ugly names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
