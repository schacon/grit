#!/bin/sh
# Ported from git/t/t9126-git-svn-follow-deleted-readded-directory.sh
# git svn fetch repository with deleted and readded directory

test_description='git svn fetch repository with deleted and readded directory'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
