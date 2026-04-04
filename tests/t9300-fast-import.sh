#!/bin/sh
# Ported from git/t/t9300-fast-import.sh
# test git fast-import utility

test_description='test git fast-import utility'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'fast-import/export — not yet ported' '
	false
'

test_done
