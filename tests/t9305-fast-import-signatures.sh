#!/bin/sh
# Ported from git/t/t9305-fast-import-signatures.sh
# git fast-import --signed-commits=<mode>

test_description='git fast-import --signed-commits=<mode>'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'fast-import/export — not yet ported' '
	false
'

test_done
