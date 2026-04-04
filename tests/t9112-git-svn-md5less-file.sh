#!/bin/sh
# Ported from git/t/t9112-git-svn-md5less-file.sh
# test that git handles an svn repository with missing md5sums

test_description='test that git handles an svn repository with missing md5sums'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
