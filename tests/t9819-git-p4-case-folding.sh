#!/bin/sh
# Ported from git/t/t9819-git-p4-case-folding.sh
# interaction with P4 case-folding

test_description='interaction with P4 case-folding'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
