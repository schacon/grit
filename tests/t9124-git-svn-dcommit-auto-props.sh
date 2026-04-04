#!/bin/sh
#
# Upstream: t9124-git-svn-dcommit-auto-props.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn dcommit honors auto-props'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
