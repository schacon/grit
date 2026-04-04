#!/bin/sh
# Ported from git/t/t9304-fast-import-marks.sh
# test exotic situations with marks

test_description='test exotic situations with marks'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'fast-import/export — not yet ported' '
	false
'

test_done
