#!/bin/sh
#
# Upstream: t9805-git-p4-skip-submit-edit.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 skipSubmitEdit config variables'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
