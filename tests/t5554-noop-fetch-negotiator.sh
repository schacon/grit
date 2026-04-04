#!/bin/sh
# Ported from git/t/t5554-noop-fetch-negotiator.sh
# test noop fetch negotiator

test_description='test noop fetch negotiator'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
