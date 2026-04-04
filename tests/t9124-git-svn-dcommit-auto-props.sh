#!/bin/sh
# Ported from git/t/t9124-git-svn-dcommit-auto-props.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn dcommit honors auto-props'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'initialize git svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'enable auto-props config (requires SVN)' '
	false
'

test_expect_failure 'add files matching auto-props (requires SVN)' '
	false
'

test_expect_failure 'disable auto-props config (requires SVN)' '
	false
'

test_expect_failure 'add files matching disabled auto-props (requires SVN)' '
	false
'

test_expect_failure 'check resulting svn repository (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'check renamed file (requires SVN)' '
	false
'

test_done
