#!/bin/sh
#
# Upstream: t9103-git-svn-tracked-directory-removed.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn tracking removed top-level path'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'make history for tracking' '
	false
'

test_expect_failure 'clone repo with git' '
	false
'

test_expect_failure 'make sure r2 still has old file' '
	false
'

test_done
