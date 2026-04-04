#!/bin/sh
#
# Upstream: t9812-git-p4-wildcards.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 wildcards'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'add p4 files with wildcards in the names' '
	false
'

test_expect_failure 'wildcard files git p4 clone' '
	false
'

test_expect_failure 'wildcard files submit back to p4, add' '
	false
'

test_expect_failure 'wildcard files submit back to p4, modify' '
	false
'

test_expect_failure 'wildcard files submit back to p4, copy' '
	false
'

test_expect_failure 'wildcard files submit back to p4, rename' '
	false
'

test_expect_failure 'wildcard files submit back to p4, delete' '
	false
'

test_expect_failure 'p4 deleted a wildcard file' '
	false
'

test_expect_failure 'wildcard files requiring keyword scrub' '
	false
'

test_done
