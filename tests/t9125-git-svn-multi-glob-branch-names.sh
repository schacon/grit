#!/bin/sh
#
# Upstream: t9125-git-svn-multi-glob-branch-names.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn multi-glob branch names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svnrepo' '
	false
'

test_expect_failure 'test clone with multi-glob in branch names' '
	false
'

test_expect_failure 'test dcommit to multi-globbed branch' '
	false
'

test_done
