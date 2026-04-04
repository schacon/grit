#!/bin/sh
# Ported from git/t/t5810-proto-disable-local.sh
# test disabling of local paths in clone/fetch

test_description='test disabling of local paths in clone/fetch'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'proto-disable — not yet ported' '
	false
'

test_done
