#!/bin/sh
#
# Upstream: t9832-unshelve.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 unshelve'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'initial clone' '
	false
'

test_expect_failure 'create shelved changelist' '
	false
'

test_expect_failure 'update shelved changelist and re-unshelve' '
	false
'

test_expect_failure 'create shelved changelist based on p4 change ahead of p4/master' '
	false
'

test_expect_failure 'try to unshelve the change' '
	false
'

test_expect_failure 'unshelve specifying the origin' '
	false
'

test_done
