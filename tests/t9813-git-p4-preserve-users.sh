#!/bin/sh
#
# Upstream: t9813-git-p4-preserve-users.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 preserve users'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
