#!/bin/sh
# Ported from git/t/t9132-git-svn-broken-symlink.sh
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

test_expect_failure '"bar" is a symlink that points to "asdf" (requires SVN)' '
	false
'

test_expect_failure 'get "bar" => symlink fix from svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure '"bar" remains a proper symlink (requires SVN)' '
	false
'

test_done
