#!/bin/sh
#
# Upstream: t9137-git-svn-dcommit-clobber-series.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn dcommit clobber series'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize repo' '
	false
'

test_expect_failure '(supposedly) non-conflicting change from SVN' '
	false
'

test_expect_failure 'some unrelated changes to git' '
	false
'

test_expect_failure 'change file but in unrelated area' '
	false
'

test_expect_failure 'attempt to dcommit with a dirty index' '
	false
'

test_done
