#!/bin/sh
# Ported from git/t/t9901-git-web--browse.sh
# git web--browse basic tests

test_description='git web--browse basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
