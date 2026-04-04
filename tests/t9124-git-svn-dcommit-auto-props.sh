#!/bin/sh
# Ported from git/t/t9124-git-svn-dcommit-auto-props.sh
# git svn dcommit honors auto-props

test_description='git svn dcommit honors auto-props'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
