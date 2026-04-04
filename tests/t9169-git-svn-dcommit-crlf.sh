#!/bin/sh
# Ported from git/t/t9169-git-svn-dcommit-crlf.sh
# git svn dcommit CRLF

test_description='git svn dcommit CRLF'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
