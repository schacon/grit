#!/bin/sh
# Ported from git/t/t9803-git-p4-shell-metachars.sh
# git p4 transparency to shell metachars in filenames

test_description='git p4 transparency to shell metachars in filenames'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
