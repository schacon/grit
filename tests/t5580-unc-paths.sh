#!/bin/sh
#
# Upstream: t5580-unc-paths.sh
# Requires UNC paths (Windows) — ported as test_expect_failure stubs.
#

test_description='various Windows-only path tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='UNC paths (Windows) not available in grit'
test_done
