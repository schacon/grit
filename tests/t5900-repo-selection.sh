#!/bin/sh
# Ported from git/t/t5900-repo-selection.sh
# selecting remote repo in ambiguous cases

test_description='selecting remote repo in ambiguous cases'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
