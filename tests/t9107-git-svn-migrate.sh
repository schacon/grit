#!/bin/sh
#
# Upstream: t9107-git-svn-migrate.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn metadata migrations from previous versions'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
