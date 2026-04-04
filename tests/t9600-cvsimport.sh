#!/bin/sh
# Ported from git/t/t9600-cvsimport.sh
# git cvsimport basic tests

test_description='git cvsimport basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'cvsimport (requires CVS) — not yet ported' '
	false
'

test_done
