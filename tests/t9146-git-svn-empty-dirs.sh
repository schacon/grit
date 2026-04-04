#!/bin/sh
#
# Upstream: t9146-git-svn-empty-dirs.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn creates empty directories'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize repo' '
	false
'

test_expect_failure 'clone' '
	false
'

test_expect_failure 'empty directories exist' '
	false
'

test_expect_failure 'option automkdirs set to false' '
	false
'

test_expect_failure 'more emptiness' '
	false
'

test_expect_failure 'git svn rebase creates empty directory' '
	false
'

test_expect_failure 'git svn mkdirs recreates empty directories' '
	false
'

test_expect_failure 'git svn mkdirs -r works' '
	false
'

test_expect_failure 'initialize trunk' '
	false
'

test_expect_failure 'clone trunk' '
	false
'

test_expect_failure 'empty directories in trunk exist' '
	false
'

test_expect_failure 'remove a top-level directory from svn' '
	false
'

test_expect_failure 'removed top-level directory does not exist' '
	false
'

test_expect_failure 'git svn gc-ed files work' '
	false
'

test_done
