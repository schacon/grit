#!/bin/sh
#
# Upstream: t9115-git-svn-dcommit-funky-renames.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn dcommit can commit renames of files with ugly names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load repository with strange names' '
	false
'

test_expect_failure 'init and fetch repository' '
	false
'

test_expect_failure 'create file in existing ugly and empty dir' '
	false
'

test_expect_failure 'rename ugly file' '
	false
'

test_expect_failure 'rename pretty file' '
	false
'

test_expect_failure 'rename pretty file into ugly one' '
	false
'

test_expect_failure 'add a file with plus signs' '
	false
'

test_expect_failure 'clone the repository to test rebase' '
	false
'

test_expect_failure 'make a commit to test rebase' '
	false
'

test_expect_failure 'git svn rebase works inside a fresh-cloned repository' '
	false
'

test_done
