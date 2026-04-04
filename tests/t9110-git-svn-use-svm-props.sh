#!/bin/sh
# Ported from git/t/t9110-git-svn-use-svm-props.sh
# git svn useSvmProps test

test_description='git svn useSvmProps test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
