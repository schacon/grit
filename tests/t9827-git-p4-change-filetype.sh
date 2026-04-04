#!/bin/sh
# Ported from git/t/t9827-git-p4-change-filetype.sh
# git p4 support for file type change

test_description='git p4 support for file type change'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
