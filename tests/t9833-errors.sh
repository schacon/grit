#!/bin/sh
#
# Upstream: t9833-errors.sh
# Requires Perforce — skipped in grit.
#

test_description='git p4 errors'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='skipping p4 tests; Perforce not available in grit'
test_done
