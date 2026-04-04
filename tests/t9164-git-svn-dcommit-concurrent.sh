#!/bin/sh
# Ported from git/t/t9164-git-svn-dcommit-concurrent.sh
# concurrent git svn dcommit

test_description='concurrent git svn dcommit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
