#!/bin/sh
# Ported from git/t/t9817-git-p4-exclude.sh
# git p4 tests for excluded paths during clone and sync

test_description='git p4 tests for excluded paths during clone and sync'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
