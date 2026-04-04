#!/bin/sh
#
# Upstream: t9821-git-p4-path-variations.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='Clone repositories with path case variations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d with case folding enabled' '
	false
'

test_expect_failure 'Create a repo with path case variations' '
	false
'

test_expect_failure 'Clone root' '
	false
'

test_expect_failure 'Clone root (ignorecase)' '
	false
'

test_expect_failure 'Clone root and ignore one file' '
	false
'

test_expect_failure 'Clone root and ignore one file (ignorecase)' '
	false
'

test_expect_failure 'Clone path' '
	false
'

test_expect_failure 'Clone path (ignorecase)' '
	false
'

test_expect_failure 'Add a new file and clone path with new file (ignorecase)' '
	false
'

test_done
