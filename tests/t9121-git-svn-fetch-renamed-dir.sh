#!/bin/sh
# Ported from git/t/t9121-git-svn-fetch-renamed-dir.sh
# git svn can fetch renamed directories

test_description='git svn can fetch renamed directories'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
