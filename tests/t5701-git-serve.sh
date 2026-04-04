#!/bin/sh
# Ported from git/t/t5701-git-serve.sh
# test protocol v2 server commands

test_description='test protocol v2 server commands'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'protocol (may require server) — not yet ported' '
	false
'

test_done
