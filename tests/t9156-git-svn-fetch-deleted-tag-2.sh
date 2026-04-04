#!/bin/sh
# Ported from git/t/t9156-git-svn-fetch-deleted-tag-2.sh
# git svn fetch deleted tag 2

test_description='git svn fetch deleted tag 2'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
