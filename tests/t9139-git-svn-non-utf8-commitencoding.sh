#!/bin/sh
#
# Upstream: t9139-git-svn-non-utf8-commitencoding.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn refuses to dcommit non-UTF8 messages'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
