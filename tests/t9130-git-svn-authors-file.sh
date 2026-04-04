#!/bin/sh
# Ported from git/t/t9130-git-svn-authors-file.sh
# git svn authors file tests

test_description='git svn authors file tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
