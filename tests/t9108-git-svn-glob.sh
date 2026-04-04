#!/bin/sh
# Ported from git/t/t9108-git-svn-glob.sh
# git svn globbing refspecs

test_description='git svn globbing refspecs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
