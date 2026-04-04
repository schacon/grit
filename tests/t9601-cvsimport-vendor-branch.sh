#!/bin/sh
#
# Upstream: t9601-cvsimport-vendor-branch.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git cvsimport handling of vendor branches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='CVS not available in grit'
test_done
