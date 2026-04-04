#!/bin/sh
# Ported from git/t/t9122-git-svn-author.sh
# git svn authorship

test_description='git svn authorship'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
