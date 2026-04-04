#!/bin/sh
# Ported from git/t/t5563-simple-http-auth.sh
# test http auth header and credential helper interop

test_description='test http auth header and credential helper interop'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
