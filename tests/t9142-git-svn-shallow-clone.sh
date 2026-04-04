#!/bin/sh
# Ported from git/t/t9142-git-svn-shallow-clone.sh
# git svn shallow clone

test_description='git svn shallow clone'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
