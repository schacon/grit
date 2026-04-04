#!/bin/sh
# Ported from git/t/t9115-git-svn-dcommit-funky-renames.sh
# git svn dcommit can commit renames of files with ugly names

test_description='git svn dcommit can commit renames of files with ugly names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
