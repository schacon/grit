#!/bin/sh
# Ported from git/t/t9159-git-svn-no-parent-mergeinfo.sh
# git svn handling of root commits in merge ranges

test_description='git svn handling of root commits in merge ranges'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
