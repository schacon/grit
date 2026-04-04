#!/bin/sh
# Ported from git/t/t9134-git-svn-ignore-paths.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn property tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup test repository (requires SVN)' '
	false
'

test_expect_failure 'clone an SVN repository with ignored www directory (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'init+fetch an SVN repository with ignored www directory (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'verify ignore-paths config saved by clone (requires SVN)' '
	false
'

test_expect_failure 'SVN-side change outside of www (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'update git svn-cloned repo (config ignore) (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'update git svn-cloned repo (option ignore) (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'SVN-side change inside of ignored www (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'update git svn-cloned repo (config ignore) (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'update git svn-cloned repo (option ignore) (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'SVN-side change in and out of ignored www (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'update git svn-cloned repo again (config ignore) (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'update git svn-cloned repo again (option ignore) (not ported - requires SVN infrastructure)' '
	false
'

test_done
