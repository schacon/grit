#!/bin/sh
# Ported from git/t/t5710-promisor-remote-capability.sh
# handling of promisor remote advertisement

test_description='handling of promisor remote advertisement'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'protocol (may require server) — not yet ported' '
	false
'

test_done
