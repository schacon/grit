#!/bin/sh
# Ported from git/t/t9115-git-svn-dcommit-funky-renames.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn dcommit can commit renames of files with ugly names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'load repository with strange names (requires SVN)' '
	false
'

test_expect_failure 'init and fetch repository (requires SVN)' '
	false
'

test_expect_failure 'create file in existing ugly and empty dir (requires SVN)' '
	false
'

test_expect_failure 'rename ugly file (requires SVN)' '
	false
'

test_expect_failure 'rename pretty file (requires SVN)' '
	false
'

test_expect_failure 'rename pretty file into ugly one (requires SVN)' '
	false
'

test_expect_failure 'add a file with plus signs (requires SVN)' '
	false
'

test_expect_failure 'clone the repository to test rebase (requires SVN)' '
	false
'

test_expect_failure 'make a commit to test rebase (requires SVN)' '
	false
'

test_expect_failure 'git svn rebase works inside a fresh-cloned repository (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'svn.pathnameencoding=cp932 new file on dcommit (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'test case 12 (requires SVN)' '
	false
'

test_done
