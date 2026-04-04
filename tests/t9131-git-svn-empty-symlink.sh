#!/bin/sh
# Ported from git/t/t9131-git-svn-empty-symlink.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='test that git handles an svn repository with empty symlinks'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'load svn dumpfile (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'clone using git svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'enable broken symlink workaround (requires SVN)' '
	false
'

test_expect_failure '"bar" is an empty file (requires SVN)' '
	false
'

test_expect_failure 'get "bar" => symlink fix from svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure '"bar" becomes a symlink (requires SVN)' '
	false
'

test_expect_failure 'clone using git svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'disable broken symlink workaround (requires SVN)' '
	false
'

test_expect_failure '"bar" is an empty file (requires SVN)' '
	false
'

test_expect_failure 'get "bar" => symlink fix from svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure '"bar" does not become a symlink (requires SVN)' '
	false
'

test_expect_failure 'clone using git svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure '"bar" is an empty file (requires SVN)' '
	false
'

test_expect_failure 'get "bar" => symlink fix from svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure '"bar" does not become a symlink (requires SVN)' '
	false
'

test_done
