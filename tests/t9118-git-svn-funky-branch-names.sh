#!/bin/sh
#
# Upstream: t9118-git-svn-funky-branch-names.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn funky branch names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svnrepo' '
	false
'

test_expect_failure 'test clone with funky branch names' '
	false
'

test_expect_failure 'test dcommit to funky branch' '
	false
'

test_expect_failure 'test dcommit to scary branch' '
	false
'

test_expect_failure 'test dcommit to trailing_dotlock branch' '
	false
'

test_done
