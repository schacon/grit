#!/bin/sh
# Ported from git/t/t9104-git-svn-follow-parent.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn fetching'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'initialize repo (requires SVN)' '
	false
'

test_expect_failure 'init and fetch a moved directory (requires SVN)' '
	false
'

test_expect_failure 'init and fetch from one svn-remote (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'follow deleted parent (requires SVN)' '
	false
'

test_expect_failure 'follow larger parent (requires SVN)' '
	false
'

test_expect_failure 'follow higher-level parent (requires SVN)' '
	false
'

test_expect_failure 'follow deleted directory (requires SVN)' '
	false
'

test_expect_failure 'follow-parent avoids deleting relevant info (requires SVN)' '
	false
'

test_expect_failure 'track initial change if it was only made to parent (requires SVN)' '
	false
'

test_expect_failure 'follow-parent is atomic (requires SVN)' '
	false
'

test_expect_failure 'track multi-parent paths (requires SVN)' '
	false
'

test_expect_failure 'multi-fetch continues to work (requires SVN)' '
	false
'

test_expect_failure 'multi-fetch works off a '\''clean'\'' repository (requires SVN)' '
	false
'

test_done
