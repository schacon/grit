#!/bin/sh
# Ported from git/t/t9811-git-p4-label-import.sh
# git p4 label tests

test_description='git p4 label tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
