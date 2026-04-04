#!/bin/sh
# Ported from git/t/t9129-git-svn-i18n-commitencoding.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn honors i18n.commitEncoding in config'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure '$H setup (requires SVN)' '
	false
'

test_expect_failure '$H commit on git side (requires SVN)' '
	false
'

test_expect_failure '$H dcommit to svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'ISO-8859-1 should match UTF-8 in svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure '$H should match UTF-8 in svn (not ported - requires SVN infrastructure)' '
	false
'

test_done
