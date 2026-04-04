#!/bin/sh
#
# Upstream: t9169-git-svn-dcommit-crlf.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn dcommit CRLF'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup commit repository' '
	false
'

test_done
