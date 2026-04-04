#!/bin/sh
#
# Upstream: t9142-git-svn-shallow-clone.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn shallow clone'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
