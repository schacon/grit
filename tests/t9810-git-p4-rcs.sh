#!/bin/sh
#
# Upstream: t9810-git-p4-rcs.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 rcs keywords'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'scrub scripts' '
	false
'

test_expect_failure 'edit far away from RCS lines' '
	false
'

test_expect_failure 'edit near RCS lines' '
	false
'

test_expect_failure 'edit keyword lines' '
	false
'

test_expect_failure 'scrub ko files differently' '
	false
'

test_expect_failure 'cleanup after failure' '
	false
'

test_expect_failure 'ktext expansion should not expand multi-line $File::' '
	false
'

test_expect_failure 'do not scrub plain text' '
	false
'

test_expect_failure 'cleanup after failure 2' '
	false
'

test_expect_failure 'add kwfile' '
	false
'

test_expect_failure 'cope with rcs keyword expansion damage' '
	false
'

test_expect_failure 'cope with rcs keyword file deletion' '
	false
'

test_expect_failure 'Add keywords in git which match the default p4 values' '
	false
'

test_expect_failure 'Add keywords in git which do not match the default p4 values' '
	false
'

test_expect_failure 'check cp1252 smart quote are preserved through RCS keyword processing' '
	false
'

test_done
