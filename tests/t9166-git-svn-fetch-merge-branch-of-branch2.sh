#!/bin/sh
# Ported from git/t/t9166-git-svn-fetch-merge-branch-of-branch2.sh
# git svn merge detection

test_description='git svn merge detection'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
