#!/bin/sh
# Ported from git/t/t9140-git-svn-reset.sh
# git svn reset

test_description='git svn reset'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
