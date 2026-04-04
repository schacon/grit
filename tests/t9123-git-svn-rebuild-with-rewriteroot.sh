#!/bin/sh
# Ported from git/t/t9123-git-svn-rebuild-with-rewriteroot.sh
# git svn respects rewriteRoot during rebuild

test_description='git svn respects rewriteRoot during rebuild'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
