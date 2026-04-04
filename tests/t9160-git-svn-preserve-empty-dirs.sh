#!/bin/sh
# Ported from git/t/t9160-git-svn-preserve-empty-dirs.sh
# git svn test (option --preserve-empty-dirs)

test_description='git svn test (option --preserve-empty-dirs)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
