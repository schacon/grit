#!/bin/sh
# Ported from git/t/t5704-protocol-violations.sh
# Test responses to violations of the network protocol. In most

test_description='Test responses to violations of the network protocol. In most'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'protocol (may require server) — not yet ported' '
	false
'

test_done
