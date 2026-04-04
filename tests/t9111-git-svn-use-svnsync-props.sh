#!/bin/sh
#
# Upstream: t9111-git-svn-use-svnsync-props.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn useSvnsyncProps test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
