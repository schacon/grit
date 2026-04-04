#!/bin/sh
#
# Upstream: t9122-git-svn-author.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn authorship'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svn repository' '
	false
'

test_expect_failure 'interact with it via git svn' '
	false
'

test_done
