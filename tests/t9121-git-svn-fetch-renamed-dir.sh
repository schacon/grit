#!/bin/sh
#
# Upstream: t9121-git-svn-fetch-renamed-dir.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn can fetch renamed directories'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
