#!/bin/sh
#
# Upstream: t9825-git-p4-handle-utf16-without-bom.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 handling of UTF-16 files without BOM'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot with UTF-16 encoded file and artificially remove BOM' '
	false
'

test_expect_failure 'clone depot with invalid UTF-16 file in verbose mode' '
	false
'

test_expect_failure 'clone depot with invalid UTF-16 file in non-verbose mode' '
	false
'

test_done
