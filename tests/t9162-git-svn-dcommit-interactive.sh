#!/bin/sh
# Ported from git/t/t9162-git-svn-dcommit-interactive.sh
# git svn dcommit --interactive series

test_description='git svn dcommit --interactive series'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
