#!/bin/sh
#
# Upstream: t9820-git-p4-editor-handling.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 handling of EDITOR'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
