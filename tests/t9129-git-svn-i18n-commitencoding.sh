#!/bin/sh
#
# Upstream: t9129-git-svn-i18n-commitencoding.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn honors i18n.commitEncoding in config'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
