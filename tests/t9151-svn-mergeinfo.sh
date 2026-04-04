#!/bin/sh
# Ported from git/t/t9151-svn-mergeinfo.sh
# git-svn svn mergeinfo properties

test_description='git-svn svn mergeinfo properties'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
