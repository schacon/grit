#!/bin/sh
# Ported from git/t/t5538-push-shallow.sh
# push from/to a shallow clone

test_description='push from/to a shallow clone'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
