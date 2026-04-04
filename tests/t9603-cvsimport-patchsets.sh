#!/bin/sh
#
# Upstream: t9603-cvsimport-patchsets.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git cvsimport testing for correct patchset estimation'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='CVS not available in grit'
test_done
