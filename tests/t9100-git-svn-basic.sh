#!/bin/sh
#
# Upstream: t9100-git-svn-basic.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'git svn --version works anywhere' '
	false
'

test_expect_failure 'git svn help works anywhere' '
	false
'

test_expect_failure 'import an SVN revision into git' '
	false
'

test_expect_failure 'exit if remote refs are ambigious' '
	false
'

test_expect_failure 'exit if init-ing a would clobber a URL' '
	false
'

test_expect_failure 'init allows us to connect to another directory in the same repo' '
	false
'

test_expect_failure 'dcommit $rev does not clobber current branch' '
	false
'

test_expect_failure 'able to dcommit to a subdirectory' '
	false
'

test_expect_failure 'dcommit should not fail with a touched file' '
	false
'

test_expect_failure 'rebase should not fail with a touched file' '
	false
'

test_expect_failure 'able to set-tree to a subdirectory' '
	false
'

test_expect_failure 'git-svn works in a bare repository' '
	false
'

test_expect_failure 'git-svn works in a repository with a gitdir: link' '
	false
'

test_expect_failure 'checkout from svn' '
	false
'

test_expect_failure '$name' '
	false
'

test_done
