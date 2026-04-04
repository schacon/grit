#!/bin/sh
#
# Upstream: t9803-git-p4-shell-metachars.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 transparency to shell metachars in filenames'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
