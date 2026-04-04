#!/bin/sh
# Ported from git/t/t5702-protocol-v2.sh
# test git wire-protocol version 2

test_description='test git wire-protocol version 2'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'protocol (may require server) — not yet ported' '
	false
'

test_done
