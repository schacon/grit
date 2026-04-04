#!/bin/sh
# Ported from git/t/t9100-git-svn-basic.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'git svn --version works anywhere (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'git svn help works anywhere (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'initialize git svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'import an SVN revision into git (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'checkout from svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'try a deep --rmdir with a commit (requires SVN)' '
	false
'

test_expect_failure 'detect node change from file to directory #1 (requires SVN)' '
	false
'

test_expect_failure 'detect node change from directory to file #1 (requires SVN)' '
	false
'

test_expect_failure 'detect node change from file to directory #2 (requires SVN)' '
	false
'

test_expect_failure 'detect node change from directory to file #2 (requires SVN)' '
	false
'

test_expect_failure 'remove executable bit from a file (requires SVN)' '
	false
'

test_expect_failure 'add executable bit back file (requires SVN)' '
	false
'

test_expect_failure 'executable file becomes a symlink to file (requires SVN)' '
	false
'

test_expect_failure 'new symlink is added to a file that was also just made executable (requires SVN)' '
	false
'

test_expect_failure 'modify a symlink to become a file (requires SVN)' '
	false
'

test_expect_failure 'commit with UTF-8 message: locale: $GIT_TEST_UTF8_LOCALE (requires SVN)' '
	false
'

test_expect_failure 'test fetch functionality (svn => git) with alternate GIT_SVN_ID (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'check imported tree checksums expected tree checksums (requires SVN)' '
	false
'

test_expect_failure 'exit if remote refs are ambigious (requires SVN)' '
	false
'

test_expect_failure 'exit if init-ing a would clobber a URL (requires SVN)' '
	false
'

test_expect_failure 'init allows us to connect to another directory in the same repo (requires SVN)' '
	false
'

test_expect_failure 'dcommit $rev does not clobber current branch (requires SVN)' '
	false
'

test_expect_failure 'able to dcommit to a subdirectory (requires SVN)' '
	false
'

test_expect_failure 'dcommit should not fail with a touched file (requires SVN)' '
	false
'

test_expect_failure 'rebase should not fail with a touched file (requires SVN)' '
	false
'

test_expect_failure 'able to set-tree to a subdirectory (requires SVN)' '
	false
'

test_expect_failure 'git-svn works in a bare repository (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'git-svn works in a repository with a gitdir: link (not ported - requires SVN infrastructure)' '
	false
'

test_done
