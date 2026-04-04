#!/bin/sh
#
# Upstream: t9830-git-p4-symlink-dir.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 symlinked directories'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'symlinked directory' '
	false
'

test_done
