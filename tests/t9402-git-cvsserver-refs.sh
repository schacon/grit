#!/bin/sh
# Ported from git/t/t9402-git-cvsserver-refs.sh
# git-cvsserver and git refspecs

test_description='git-cvsserver and git refspecs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-cvsserver (requires CVS) — not yet ported' '
	false
'

test_done
