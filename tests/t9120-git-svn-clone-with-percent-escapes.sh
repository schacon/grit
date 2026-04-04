#!/bin/sh
# Ported from git/t/t9120-git-svn-clone-with-percent-escapes.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn clone with percent escapes'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup svnrepo (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'test clone with percent escapes (requires SVN)' '
	false
'

test_expect_failure 'svn checkout with percent escapes (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'svn checkout with space (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'test clone trunk with percent escapes and minimize-url (requires SVN)' '
	false
'

test_expect_failure 'test clone trunk with percent escapes (requires SVN)' '
	false
'

test_expect_failure 'test clone --stdlayout with percent escapes (requires SVN)' '
	false
'

test_expect_failure 'test clone -s with unescaped space (requires SVN)' '
	false
'

test_done
