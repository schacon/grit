#!/bin/sh
#
# Upstream: t9400-git-cvsserver-server.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git-cvsserver access

tests read access to a git repository with the
cvs CLI client via git-cvsserver server'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='CVS not available in grit'
test_done
