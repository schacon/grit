#!/bin/sh
# Ported from git/t/t9111-git-svn-use-svnsync-props.sh
# git svn useSvnsyncProps test

test_description='git svn useSvnsyncProps test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
