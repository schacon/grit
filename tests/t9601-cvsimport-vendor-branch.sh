#!/bin/sh
# Ported from git/t/t9601-cvsimport-vendor-branch.sh
# cvsimport-vendor-branch

test_description='cvsimport-vendor-branch'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'cvsimport (requires CVS) — not yet ported' '
	false
'

test_done
