#!/bin/sh
# Ported from git/t/t5533-push-cas.sh
# compare & swap push force/delete safety

test_description='compare & swap push force/delete safety'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
