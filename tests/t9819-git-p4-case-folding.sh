#!/bin/sh
#
# Upstream: t9819-git-p4-case-folding.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='interaction with P4 case-folding'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d with case folding enabled' '
	false
'

test_expect_failure 'Create a repo, name is lowercase' '
	false
'

test_expect_failure 'Check p4 is in case-folding mode' '
	false
'

test_expect_failure 'Clone lc repo using lc name' '
	false
'

test_expect_failure 'Clone lc repo using uc name' '
	false
'

test_expect_failure 'Clone UC repo with lc name' '
	false
'

test_done
