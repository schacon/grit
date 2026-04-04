#!/bin/sh
#
# Upstream: t9160-git-svn-preserve-empty-dirs.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn test (option --preserve-empty-dirs)

This test uses git to clone a Subversion repository that contains empty
directories, and checks that corresponding directories are created in the
local Git repository with placeholder files.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize source svn repo containing empty dirs' '
	false
'

test_expect_failure 'clone svn repo with --preserve-empty-dirs' '
	false
'

test_expect_failure 'directory empty from inception' '
	false
'

test_expect_failure 'directory empty from subsequent svn commit' '
	false
'

test_expect_failure 'add entry to previously empty directory' '
	false
'

test_expect_failure 'remove non-last entry from directory' '
	false
'

test_expect_failure 'clone svn repo with --placeholder-file specified' '
	false
'

test_expect_failure 'placeholder namespace conflict with file' '
	false
'

test_expect_failure 'placeholder namespace conflict with directory' '
	false
'

test_expect_failure 'second set of svn commits and rebase' '
	false
'

test_expect_failure 'flag persistence during subsequent rebase' '
	false
'

test_expect_failure 'placeholder list persistence during subsequent rebase' '
	false
'

test_done
