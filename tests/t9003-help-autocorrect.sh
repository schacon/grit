#!/bin/sh
# Ported from git/t/t9003-help-autocorrect.sh
# help.autocorrect finding a match

test_description='help.autocorrect finding a match'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
