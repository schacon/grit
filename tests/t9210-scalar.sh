#!/bin/sh
# Ported from git/t/t9210-scalar.sh
# test the `scalar` command

test_description='test the `scalar` command'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
