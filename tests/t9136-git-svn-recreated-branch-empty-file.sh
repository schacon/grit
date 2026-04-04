#!/bin/sh
# Ported from git/t/t9136-git-svn-recreated-branch-empty-file.sh
# test recreated svn branch with empty files

test_description='test recreated svn branch with empty files'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
