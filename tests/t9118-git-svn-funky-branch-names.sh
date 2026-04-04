#!/bin/sh
# Ported from git/t/t9118-git-svn-funky-branch-names.sh
# git svn funky branch names

test_description='git svn funky branch names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
