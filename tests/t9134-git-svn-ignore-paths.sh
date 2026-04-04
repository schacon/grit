#!/bin/sh
#
# Upstream: t9134-git-svn-ignore-paths.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn property tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup test repository' '
	false
'

test_expect_failure 'clone an SVN repository with ignored www directory' '
	false
'

test_expect_failure 'init+fetch an SVN repository with ignored www directory' '
	false
'

test_expect_failure 'verify ignore-paths config saved by clone' '
	false
'

test_expect_failure 'SVN-side change outside of www' '
	false
'

test_expect_failure 'update git svn-cloned repo (config ignore)' '
	false
'

test_expect_failure 'update git svn-cloned repo (option ignore)' '
	false
'

test_expect_failure 'SVN-side change inside of ignored www' '
	false
'

test_expect_failure 'update git svn-cloned repo (config ignore)' '
	false
'

test_expect_failure 'update git svn-cloned repo (option ignore)' '
	false
'

test_expect_failure 'SVN-side change in and out of ignored www' '
	false
'

test_expect_failure 'update git svn-cloned repo again (config ignore)' '
	false
'

test_expect_failure 'update git svn-cloned repo again (option ignore)' '
	false
'

test_done
