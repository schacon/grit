#!/bin/sh
#
# Upstream: t9200-git-cvsexportcommit.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='Test export of commits to CVS'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='CVS not available in grit'
test_done
