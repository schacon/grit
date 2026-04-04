#!/bin/sh
#
# Upstream: t9133-git-svn-nested-git-repo.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn property tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup repo with a git repo inside it' '
	false
'

test_expect_failure 'clone an SVN repo containing a git repo' '
	false
'

test_expect_failure 'SVN-side change outside of .git' '
	false
'

test_expect_failure 'update git svn-cloned repo' '
	false
'

test_expect_failure 'SVN-side change inside of .git' '
	false
'

test_expect_failure 'update git svn-cloned repo' '
	false
'

test_expect_failure 'SVN-side change in and out of .git' '
	false
'

test_expect_failure 'update git svn-cloned repo again' '
	false
'

test_done
