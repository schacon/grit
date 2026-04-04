#!/bin/sh
# Ported from git/t/t5815-submodule-protos.sh
# test protocol filtering with submodules

test_description='test protocol filtering with submodules'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'proto-disable — not yet ported' '
	false
'

test_done
