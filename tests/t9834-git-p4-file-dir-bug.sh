#!/bin/sh
#
# Upstream: t9834-git-p4-file-dir-bug.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 directory/file bug handling

This test creates files and directories with the same name in perforce and
checks that git-p4 recovers from the error at the same time as the perforce
repository.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'clone with git-p4' '
	false
'

test_expect_failure 'check contents' '
	false
'

test_expect_failure 'rebase and check empty' '
	false
'

test_done
