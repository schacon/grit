#!/bin/sh
# Ported from git/t/t9816-git-p4-locked.sh
# git p4 locked file behavior

test_description='git p4 locked file behavior'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
