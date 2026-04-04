#!/bin/sh
# Ported from git/t/t9152-svn-empty-dirs-after-gc.sh
# git svn creates empty directories, calls git gc, makes sure they are still empty

test_description='git svn creates empty directories, calls git gc, makes sure they are still empty'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
