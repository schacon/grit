#!/bin/sh
# Ported from git/t/t5534-push-signed.sh
# signed push

test_description='signed push'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
