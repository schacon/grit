#!/bin/sh
# Ported from git/t/t9125-git-svn-multi-glob-branch-names.sh
# git svn multi-glob branch names

test_description='git svn multi-glob branch names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
