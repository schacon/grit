#!/bin/sh
#
# Upstream: t9127-git-svn-partial-rebuild.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn partial-rebuild tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize svnrepo' '
	false
'

test_expect_failure 'import an early SVN revision into git' '
	false
'

test_expect_failure 'make full git mirror of SVN' '
	false
'

test_expect_failure 'fetch from git mirror and partial-rebuild' '
	false
'

test_done
