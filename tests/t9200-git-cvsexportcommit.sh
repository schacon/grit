#!/bin/sh
# Ported from git/t/t9200-git-cvsexportcommit.sh
# Test export of commits to CVS

test_description='Test export of commits to CVS'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
