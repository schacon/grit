#!/bin/sh
# Ported from git/t/t9144-git-svn-old-rev_map.sh
# git svn old rev_map preservd

test_description='git svn old rev_map preservd'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
