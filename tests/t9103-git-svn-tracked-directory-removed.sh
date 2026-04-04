#!/bin/sh
# Ported from git/t/t9103-git-svn-tracked-directory-removed.sh
# git svn tracking removed top-level path

test_description='git svn tracking removed top-level path'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
