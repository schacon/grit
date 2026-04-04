#!/bin/sh
# Ported from git/t/t9400-git-cvsserver-server.sh
# git-cvsserver access

test_description='git-cvsserver access'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-cvsserver (requires CVS) — not yet ported' '
	false
'

test_done
