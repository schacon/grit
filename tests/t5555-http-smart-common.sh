#!/bin/sh
# Ported from git/t/t5555-http-smart-common.sh
# test functionality common to smart fetch & push

test_description='test functionality common to smart fetch & push'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
