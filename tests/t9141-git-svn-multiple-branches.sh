#!/bin/sh
# Ported from git/t/t9141-git-svn-multiple-branches.sh
# git svn multiple branch and tag paths in the svn repo

test_description='git svn multiple branch and tag paths in the svn repo'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
