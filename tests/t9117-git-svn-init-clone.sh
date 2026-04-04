#!/bin/sh
# Ported from git/t/t9117-git-svn-init-clone.sh
# git svn init/clone tests

test_description='git svn init/clone tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
