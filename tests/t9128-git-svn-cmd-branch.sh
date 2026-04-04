#!/bin/sh
# Ported from git/t/t9128-git-svn-cmd-branch.sh
# git svn partial-rebuild tests

test_description='git svn partial-rebuild tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
