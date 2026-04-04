#!/bin/sh
# Ported from git/t/t5705-session-id-in-capabilities.sh
# session ID in capabilities

test_description='session ID in capabilities'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'protocol (may require server) — not yet ported' '
	false
'

test_done
