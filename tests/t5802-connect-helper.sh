#!/bin/sh
# Ported from git/t/t5802-connect-helper.sh
# ext::cmd remote 

test_description='ext::cmd remote '

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'proto-disable — not yet ported' '
	false
'

test_done
