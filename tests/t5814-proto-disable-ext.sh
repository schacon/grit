#!/bin/sh
# Ported from git/t/t5814-proto-disable-ext.sh
# test disabling of remote-helper paths in clone/fetch

test_description='test disabling of remote-helper paths in clone/fetch'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'proto-disable — not yet ported' '
	false
'

test_done
