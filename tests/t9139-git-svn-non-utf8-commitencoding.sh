#!/bin/sh
# Ported from git/t/t9139-git-svn-non-utf8-commitencoding.sh
# git svn refuses to dcommit non-UTF8 messages

test_description='git svn refuses to dcommit non-UTF8 messages'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
