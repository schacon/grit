#!/bin/sh
# Ported from git/t/t9137-git-svn-dcommit-clobber-series.sh
# git svn dcommit clobber series

test_description='git svn dcommit clobber series'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
