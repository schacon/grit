#!/bin/sh
# Ported from git/t/t9133-git-svn-nested-git-repo.sh
# git svn property tests

test_description='git svn property tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
