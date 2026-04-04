#!/bin/sh
# Ported from git/t/t9401-git-cvsserver-crlf.sh
# git-cvsserver -kb modes

test_description='git-cvsserver -kb modes'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-cvsserver (requires CVS) — not yet ported' '
	false
'

test_done
