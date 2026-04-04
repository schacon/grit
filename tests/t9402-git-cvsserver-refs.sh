#!/bin/sh
#
# Upstream: t9402-git-cvsserver-refs.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git-cvsserver and git refspecs

tests ability for git-cvsserver to switch between and compare
tags, branches and other git refspecs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='CVS not available in grit'
test_done
