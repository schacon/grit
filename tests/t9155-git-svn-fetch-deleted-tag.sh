#!/bin/sh
#
# Upstream: t9155-git-svn-fetch-deleted-tag.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn fetch deleted tag'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svn repo' '
	false
'

test_expect_failure 'fetch deleted tags from same revision with checksum error' '
	false
'

test_done
