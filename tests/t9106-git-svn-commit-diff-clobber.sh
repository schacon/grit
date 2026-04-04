#!/bin/sh
# Ported from git/t/t9106-git-svn-commit-diff-clobber.sh
# git svn commit-diff clobber

test_description='git svn commit-diff clobber'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
