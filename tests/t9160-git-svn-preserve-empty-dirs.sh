#!/bin/sh
#
# Upstream: t9160-git-svn-preserve-empty-dirs.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn test (option --preserve-empty-dirs)

This test uses git to clone a Subversion repository that contains empty
directories, and checks that corresponding directories are created in the
local Git repository with placeholder files.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
