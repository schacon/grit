#!/bin/sh
# Ported from git/t/t9350-fast-export.sh
# git fast-export

test_description='git fast-export'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'fast-import/export — not yet ported' '
	false
'

test_done
