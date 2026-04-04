#!/bin/sh
# Ported from git/t/t5535-fetch-push-symref.sh
# avoiding conflicting update through symref aliasing

test_description='avoiding conflicting update through symref aliasing'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
