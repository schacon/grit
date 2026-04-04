#!/bin/sh
# Ported from git/t/t9102-git-svn-deep-rmdir.sh
# git svn rmdir

test_description='git svn rmdir'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
