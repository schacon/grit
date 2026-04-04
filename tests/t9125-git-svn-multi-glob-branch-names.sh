#!/bin/sh
# Ported from git/t/t9125-git-svn-multi-glob-branch-names.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn multi-glob branch names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup svnrepo (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'test clone with multi-glob in branch names (requires SVN)' '
	false
'

test_expect_failure 'test dcommit to multi-globbed branch (requires SVN)' '
	false
'

test_done
