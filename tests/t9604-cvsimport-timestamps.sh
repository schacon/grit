#!/bin/sh
#
# Upstream: t9604-cvsimport-timestamps.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git cvsimport timestamps'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- CVS not available in grit ---

test_expect_failure 'check timestamps are UTC' '
	false
'

test_expect_failure 'check timestamps with author-specific timezones' '
	false
'

test_done
