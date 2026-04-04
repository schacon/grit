#!/bin/sh
# Ported from git/t/t5559-http-fetch-smart-http2.sh
# http-fetch-smart-http2

test_description='http-fetch-smart-http2'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
