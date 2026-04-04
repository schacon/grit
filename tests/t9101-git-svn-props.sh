#!/bin/sh
# Ported from git/t/t9101-git-svn-props.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn property tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'checkout working copy from svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'setup some commits to svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'initialize git svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'fetch revisions from svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'test svn:keywords ignoring (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'raw $Id$ found in kw.c (requires SVN)' '
	false
'

test_expect_failure 'propset CR on crlf files (requires SVN)' '
	false
'

test_expect_failure 'fetch and pull latest from svn and checkout a new wc (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'Comparing $i (requires SVN)' '
	false
'

test_expect_failure 'Set CRLF on cr files (requires SVN)' '
	false
'

test_expect_failure 'fetch and pull latest from svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'CRLF + $Id$ (requires SVN)' '
	false
'

test_expect_failure 'CRLF + $Id$ (no newline) (requires SVN)' '
	false
'

test_expect_failure 'test show-ignore (requires SVN)' '
	false
'

test_expect_failure 'test create-ignore (requires SVN)' '
	false
'

test_expect_failure 'test propget (requires SVN)' '
	false
'

test_expect_failure 'test proplist (requires SVN)' '
	false
'

test_done
