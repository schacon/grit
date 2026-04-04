#!/bin/sh
# Ported from git/t/t9211-scalar-clone.sh
# test the `scalar clone` subcommand

test_description='test the `scalar clone` subcommand'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
