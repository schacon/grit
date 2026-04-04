#!/bin/sh
# Ported from git/t/t9143-git-svn-gc.sh
# git svn gc basic tests

test_description='git svn gc basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
