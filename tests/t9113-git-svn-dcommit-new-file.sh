#!/bin/sh
# Ported from git/t/t9113-git-svn-dcommit-new-file.sh
# git svn dcommit new files over svn:// test

test_description='git svn dcommit new files over svn:// test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
