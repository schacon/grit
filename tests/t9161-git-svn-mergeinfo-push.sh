#!/bin/sh
# Ported from git/t/t9161-git-svn-mergeinfo-push.sh
# git-svn svn mergeinfo propagation

test_description='git-svn svn mergeinfo propagation'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
