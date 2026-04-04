#!/bin/sh
# Ported from git/t/t9603-cvsimport-patchsets.sh
# git cvsimport testing for correct patchset estimation

test_description='git cvsimport testing for correct patchset estimation'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'cvsimport (requires CVS) — not yet ported' '
	false
'

test_done
