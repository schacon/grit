#!/bin/sh
# Ported from git/t/t9107-git-svn-migrate.sh
# git svn metadata migrations from previous versions

test_description='git svn metadata migrations from previous versions'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
