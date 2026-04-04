#!/bin/sh
# Ported from git/t/t9129-git-svn-i18n-commitencoding.sh
# git svn honors i18n.commitEncoding in config

test_description='git svn honors i18n.commitEncoding in config'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-svn (requires Subversion) — not yet ported' '
	false
'

test_done
