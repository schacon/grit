#!/bin/sh
#
# Upstream: t9147-git-svn-include-paths.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn property tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup test repository' '
	false
'

test_expect_failure 'clone an SVN repository with filter to include qqq directory' '
	false
'

test_expect_failure 'init+fetch an SVN repository with included qqq directory' '
	false
'

test_expect_failure 'verify include-paths config saved by clone' '
	false
'

test_expect_failure 'SVN-side change outside of www' '
	false
'

test_expect_failure 'update git svn-cloned repo (config include)' '
	false
'

test_expect_failure 'update git svn-cloned repo (option include)' '
	false
'

test_expect_failure 'SVN-side change inside of ignored www' '
	false
'

test_expect_failure 'update git svn-cloned repo (config include)' '
	false
'

test_expect_failure 'update git svn-cloned repo (option include)' '
	false
'

test_expect_failure 'SVN-side change in and out of included qqq' '
	false
'

test_expect_failure 'update git svn-cloned repo again (config include)' '
	false
'

test_expect_failure 'update git svn-cloned repo again (option include)' '
	false
'

test_done
