#!/bin/sh
# Ported from git/t/t9107-git-svn-migrate.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn metadata migrations from previous versions'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup old-looking metadata (requires SVN)' '
	false
'

test_expect_failure 'git-svn-HEAD is a real HEAD (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'initialize old-style (v0) git svn layout (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'initialize a multi-repository repo (requires SVN)' '
	false
'

test_expect_failure 'multi-fetch works on partial urls + paths (requires SVN)' '
	false
'

test_expect_failure 'migrate --minimize on old inited layout (requires SVN)' '
	false
'

test_expect_failure '.rev_db auto-converted to .rev_map.UUID (requires SVN)' '
	false
'

test_done
