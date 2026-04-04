#!/bin/sh
# Ported from git/t/t9120-git-svn-clone-with-percent-escapes.sh
# git svn clone with percent escapes

test_description='git svn clone with percent escapes'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
