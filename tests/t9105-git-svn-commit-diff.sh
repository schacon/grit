#!/bin/sh
# Ported from git/t/t9105-git-svn-commit-diff.sh
# git svn commit-diff

test_description='git svn commit-diff'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
