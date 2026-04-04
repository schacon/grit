#!/bin/sh
# Ported from git/t/t9119-git-svn-info.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn info'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup repository and import (requires SVN)' '
	false
'

test_expect_failure 'info (requires SVN)' '
	false
'

test_expect_failure 'info --url (requires SVN)' '
	false
'

test_expect_failure 'info . (requires SVN)' '
	false
'

test_expect_failure 'info $(pwd) (requires SVN)' '
	false
'

test_expect_failure 'info $(pwd)/../___wc (requires SVN)' '
	false
'

test_expect_failure 'info $(pwd)/../___wc//file (requires SVN)' '
	false
'

test_expect_failure 'info --url . (requires SVN)' '
	false
'

test_expect_failure 'info file (requires SVN)' '
	false
'

test_expect_failure 'info --url file (requires SVN)' '
	false
'

test_expect_failure 'info directory (requires SVN)' '
	false
'

test_expect_failure 'info inside directory (requires SVN)' '
	false
'

test_expect_failure 'info --url directory (requires SVN)' '
	false
'

test_expect_failure 'info symlink-file (requires SVN)' '
	false
'

test_expect_failure 'info --url symlink-file (requires SVN)' '
	false
'

test_expect_failure 'info symlink-directory (requires SVN)' '
	false
'

test_expect_failure 'info --url symlink-directory (requires SVN)' '
	false
'

test_expect_failure 'info added-file (requires SVN)' '
	false
'

test_expect_failure 'info --url added-file (requires SVN)' '
	false
'

test_expect_failure 'info added-directory (requires SVN)' '
	false
'

test_expect_failure 'info --url added-directory (requires SVN)' '
	false
'

test_expect_failure 'info added-symlink-file (requires SVN)' '
	false
'

test_expect_failure 'info --url added-symlink-file (requires SVN)' '
	false
'

test_expect_failure 'info added-symlink-directory (requires SVN)' '
	false
'

test_expect_failure 'info --url added-symlink-directory (requires SVN)' '
	false
'

test_expect_failure 'info deleted-file (requires SVN)' '
	false
'

test_expect_failure 'info --url file (deleted) (requires SVN)' '
	false
'

test_expect_failure 'info deleted-directory (requires SVN)' '
	false
'

test_expect_failure 'info --url directory (deleted) (requires SVN)' '
	false
'

test_expect_failure 'info deleted-symlink-file (requires SVN)' '
	false
'

test_expect_failure 'info --url symlink-file (deleted) (requires SVN)' '
	false
'

test_expect_failure 'info deleted-symlink-directory (requires SVN)' '
	false
'

test_expect_failure 'info --url symlink-directory (deleted) (requires SVN)' '
	false
'

test_expect_failure 'info unknown-file (requires SVN)' '
	false
'

test_expect_failure 'info --url unknown-file (requires SVN)' '
	false
'

test_expect_failure 'info unknown-directory (requires SVN)' '
	false
'

test_expect_failure 'info --url unknown-directory (requires SVN)' '
	false
'

test_expect_failure 'info unknown-symlink-file (requires SVN)' '
	false
'

test_expect_failure 'info --url unknown-symlink-file (requires SVN)' '
	false
'

test_expect_failure 'info unknown-symlink-directory (requires SVN)' '
	false
'

test_expect_failure 'info --url unknown-symlink-directory (requires SVN)' '
	false
'

test_done
