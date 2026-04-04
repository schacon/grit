#!/bin/sh
# Ported from git/t/t9303-fast-import-compression.sh
# compression setting of fast-import utility

test_description='compression setting of fast-import utility'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'fast-import/export — not yet ported' '
	false
'

test_done
