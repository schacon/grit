#!/bin/sh
# Ported from git/t/t9145-git-svn-master-branch.sh
# git svn initial main branch is 

test_description='git svn initial main branch is '

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
