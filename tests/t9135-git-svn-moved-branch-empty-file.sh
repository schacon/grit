#!/bin/sh
# Ported from git/t/t9135-git-svn-moved-branch-empty-file.sh
# test moved svn branch with missing empty files

test_description='test moved svn branch with missing empty files'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
