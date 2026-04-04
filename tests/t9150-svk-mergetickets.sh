#!/bin/sh
# Ported from git/t/t9150-svk-mergetickets.sh
# git-svn svk merge tickets

test_description='git-svn svk merge tickets'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
