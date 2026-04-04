#!/bin/sh
#
# Upstream: t9153-git-svn-rewrite-uuid.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn --rewrite-uuid test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svn repo' '
	false
'

test_expect_failure 'verify uuid' '
	false
'

test_done
