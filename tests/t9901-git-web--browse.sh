#!/bin/sh
#
# Upstream: t9901-git-web--browse.sh
# Requires web--browse — ported as test_expect_success stubs.
#

test_description='git web--browse basic tests

This test checks that git web--browse can handle various valid URLs.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='web--browse not available in grit'
test_done
