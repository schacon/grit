#!/bin/sh
#
# Upstream: t9814-git-p4-rename.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 rename'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure '"p4 help unknown" errors out' '
	false
'

test_expect_failure 'create files' '
	false
'

test_expect_failure 'detect renames' '
	false
'

test_expect_failure 'detect copies' '
	false
'

test_done
