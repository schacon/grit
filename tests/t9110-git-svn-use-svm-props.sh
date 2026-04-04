#!/bin/sh
#
# Upstream: t9110-git-svn-use-svm-props.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn useSvmProps test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
