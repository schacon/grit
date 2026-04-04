#!/bin/sh
# Ported from git/t/t9806-git-p4-options.sh
# git p4 options

test_description='git p4 options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
