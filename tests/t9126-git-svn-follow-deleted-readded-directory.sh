#!/bin/sh
#
# Upstream: t9126-git-svn-follow-deleted-readded-directory.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn fetch repository with deleted and readded directory'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load repository' '
	false
'

test_expect_failure 'fetch repository' '
	false
'

test_done
