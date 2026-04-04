#!/bin/sh
# Ported from git/t/t9130-git-svn-authors-file.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn authors file tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup svnrepo (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'start import with incomplete authors file (requires SVN)' '
	false
'

test_expect_failure 'imported 2 revisions successfully (requires SVN)' '
	false
'

test_expect_failure 'continues to import once authors have been added (requires SVN)' '
	false
'

test_expect_failure 'authors-file against globs (requires SVN)' '
	false
'

test_expect_failure 'fetch fails on ee (requires SVN)' '
	false
'

test_expect_failure 'failure happened without negative side effects (requires SVN)' '
	false
'

test_expect_failure 'fetch continues after authors-file is fixed (requires SVN)' '
	false
'

test_expect_failure 'test case 9 (requires SVN)' '
	false
'

test_expect_failure 'authors-file imported user without email (requires SVN)' '
	false
'

test_done
