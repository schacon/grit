#!/bin/sh
#
# Upstream: t9602-cvsimport-branches-tags.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git cvsimport handling of branches and tags'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='CVS not available in grit'
test_done
