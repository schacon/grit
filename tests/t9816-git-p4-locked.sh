#!/bin/sh
#
# Upstream: t9816-git-p4-locked.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 locked file behavior'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'edit with lock not taken' '
	false
'

test_expect_failure 'add with lock not taken' '
	false
'

test_expect_failure 'edit with lock taken' '
	false
'

test_expect_failure 'delete with lock taken' '
	false
'

test_expect_failure 'chmod with lock taken' '
	false
'

test_expect_failure 'copy with lock taken' '
	false
'

test_expect_failure 'move with lock taken' '
	false
'

test_done
