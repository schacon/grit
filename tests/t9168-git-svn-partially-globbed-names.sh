#!/bin/sh
# Ported from git/t/t9168-git-svn-partially-globbed-names.sh
# git svn globbing refspecs with prefixed globs

test_description='git svn globbing refspecs with prefixed globs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
