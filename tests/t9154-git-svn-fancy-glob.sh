#!/bin/sh
# Ported from git/t/t9154-git-svn-fancy-glob.sh
# git svn fancy glob test

test_description='git svn fancy glob test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
