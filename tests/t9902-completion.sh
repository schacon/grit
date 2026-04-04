#!/bin/sh
# Ported from git/t/t9902-completion.sh
# test bash completion

test_description='test bash completion'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
