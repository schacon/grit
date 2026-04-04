#!/bin/sh
#
# Upstream: t9825-git-p4-handle-utf16-without-bom.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 handling of UTF-16 files without BOM'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
