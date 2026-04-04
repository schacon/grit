#!/bin/sh
#
# Upstream: t9822-git-p4-path-encoding.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='Clone repositories with non ASCII paths'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
