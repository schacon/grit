#!/bin/sh
#
# Upstream: t9833-errors.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 errors'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'add p4 files' '
	false
'

test_expect_failure 'error handling' '
	false
'

test_expect_failure 'ticket logged out' '
	false
'

test_done
