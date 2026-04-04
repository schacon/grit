#!/bin/sh
#
# Upstream: t9401-git-cvsserver-crlf.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git-cvsserver -kb modes

tests -kb mode for binary files when accessing a git
repository using cvs CLI client via git-cvsserver server'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='CVS not available in grit'
test_done
